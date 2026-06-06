use std::sync::Arc;

use lsp_types::{FileChangeType, FileEvent, Url};
use witcherscript_language::diagnostics::{
    collect_base_script_conflict_diagnostics, collect_duplicate_symbol_diagnostics,
};

use crate::config::{Config, DiagnosticsScope};

use super::legacy_helpers::{make_backend, make_game_dir, write_script, LocalTempDir};

#[tokio::test]
async fn matching_legacy_file_shadows_base_and_lands_in_workspace() {
    let temp = LocalTempDir::new("ws_matching_legacy_file_drops_base");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let legacy_path = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );
    let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir.clone()];
    });

    backend.index_base_scripts().await;

    let snap = backend.snapshot();
    let base_docs = &snap.base_scripts_documents;
    assert!(
        base_docs.contains_key(base_url.as_str()),
        "base script must stay in the base index for references; got keys {:?}",
        base_docs.keys().collect::<Vec<_>>()
    );
    assert!(
        backend
            .snapshot()
            .suppressed_base_uris
            .contains(base_url.as_str()),
        "overridden base URI must be in suppressed_base_uris",
    );

    let ws_docs = &snap.workspace_documents;
    assert!(
        ws_docs.contains_key(legacy_url.as_str()),
        "legacy file should be in workspace_documents; got keys {:?}",
        ws_docs.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn mod_shared_imports_override_shadows_base_and_lands_in_workspace() {
    let temp = LocalTempDir::new("ws_mod_shared_imports_override");
    let (game_dir, base_url) = make_game_dir(
        temp.path(),
        "local/CDestructionComponent.ws",
        "class CDestructionComponent {}\n",
    );
    let override_path = write_script(
        &game_dir.join("Mods").join("modSharedImports"),
        "content/scripts/local/CDestructionComponent.ws",
        "class CDestructionComponent {}\n// shared imports\n",
    );
    let override_url = Url::from_file_path(&override_path).expect("override path -> url");

    let backend = make_backend();
    backend.update_config(|c| c.game_directory = Some(game_dir));

    backend.index_base_scripts().await;

    assert!(
        backend
            .snapshot()
            .base_scripts_documents
            .contains_key(base_url.as_str()),
        "a modSharedImports replacement must keep the base script in the base index"
    );
    assert!(
        backend
            .snapshot()
            .suppressed_base_uris
            .contains(base_url.as_str()),
        "a modSharedImports replacement must suppress the base URI in resolution",
    );
    assert!(
        backend
            .snapshot()
            .workspace_documents
            .contains_key(override_url.as_str()),
        "a modSharedImports replacement script must land in workspace_documents"
    );
}

#[tokio::test]
async fn mod_shared_imports_skipped_when_auto_load_off() {
    let temp = LocalTempDir::new("ws_mod_shared_imports_auto_off");
    let (game_dir, base_url) = make_game_dir(
        temp.path(),
        "local/CDestructionComponent.ws",
        "class CDestructionComponent {}\n",
    );
    let override_path = write_script(
        &game_dir.join("Mods").join("modSharedImports"),
        "content/scripts/local/CDestructionComponent.ws",
        "class CDestructionComponent {}\n// shared imports\n",
    );
    let override_url = Url::from_file_path(&override_path).expect("override path -> url");

    let backend = make_backend();
    backend.config.store(Arc::new(Config {
        game_directory: Some(game_dir),
        auto_load_mod_shared_imports: false,
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));

    backend.index_base_scripts().await;

    assert!(
        backend
            .snapshot()
            .base_scripts_documents
            .contains_key(base_url.as_str()),
        "with auto-load off the base script must stay in the base index"
    );
    assert!(
        !backend
            .snapshot()
            .workspace_documents
            .contains_key(override_url.as_str()),
        "with auto-load off the modSharedImports script must not be indexed"
    );
}

#[tokio::test]
async fn deleting_a_legacy_file_removes_it_from_the_workspace() {
    let temp = LocalTempDir::new("ws_deleting_legacy_file_removes_it");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let legacy_path = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );
    let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir];
    });

    backend.index_base_scripts().await;
    assert!(
        backend
            .snapshot()
            .workspace_documents
            .contains_key(legacy_url.as_str()),
        "legacy file should be indexed into the workspace first"
    );

    std::fs::remove_file(&legacy_path).expect("remove legacy file");
    backend.index_base_scripts().await;

    assert!(
        !backend
            .snapshot()
            .workspace_documents
            .contains_key(legacy_url.as_str()),
        "a deleted legacy file must not linger in workspace_documents"
    );
    assert!(
        backend
            .snapshot()
            .base_scripts_documents
            .contains_key(base_url.as_str()),
        "the base script returns to the base index once nothing overrides it"
    );
}

#[tokio::test]
async fn unmatched_legacy_file_still_lands_in_workspace() {
    let temp = LocalTempDir::new("ws_unmatched_legacy_file_lands_in_workspace");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let legacy_path = write_script(
        &legacy_dir,
        "game/MyMod.ws",
        "@addMethod(CR4Player)\nfunction Hi() {}\n",
    );
    let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir.clone()];
    });

    backend.index_base_scripts().await;

    let snap = backend.snapshot();
    let base_docs = &snap.base_scripts_documents;
    assert!(
        base_docs.contains_key(base_url.as_str()),
        "unmatched legacy file must not remove the base script"
    );

    let ws_docs = &snap.workspace_documents;
    assert!(
        ws_docs.contains_key(legacy_url.as_str()),
        "annotated legacy file should be in workspace_documents"
    );
}

#[tokio::test]
async fn base_script_conflict_silent_on_matched_legacy_file() {
    let temp = LocalTempDir::new("ws_base_script_conflict_silent_on_matched_legacy");
    let (game_dir, _base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let _ = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir];
    });

    backend.index_base_scripts().await;

    let snap = backend.snapshot();
    let ws = snap.workspace_index.as_ref();
    let base = snap.base_scripts_index.as_ref();
    let legacy_dirs = backend.config.load().legacy_script_dirs.clone();
    let diagnostics = collect_base_script_conflict_diagnostics(ws, base, &legacy_dirs);
    assert!(
        diagnostics.is_empty(),
        "matched legacy file must not trigger base_script_conflict; got {diagnostics:?}",
    );
}

#[tokio::test]
async fn opening_an_overridden_base_script_keeps_it_out_of_the_workspace() {
    let temp = LocalTempDir::new("ws_open_overridden_base_no_dup");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let _ = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir];
    });
    backend.index_base_scripts().await;

    backend.update_open_document(base_url.clone(), "class CR4Player {}\n".to_string());

    let snap = backend.snapshot();
    let ws = snap.workspace_index.as_ref();
    let base = snap.base_scripts_index.as_ref();
    let legacy_dirs = backend.config.load().legacy_script_dirs.clone();

    assert!(
        collect_duplicate_symbol_diagnostics(ws).is_empty(),
        "opening the overridden base script must not create a workspace duplicate",
    );
    assert!(
        collect_base_script_conflict_diagnostics(ws, base, &legacy_dirs).is_empty(),
        "the legacy override must not be flagged once both files are loaded",
    );
    assert!(
        base.documents().any(|(uri, _)| uri == base_url.as_str()),
        "the opened base script should be indexed as a base script",
    );
}

#[tokio::test]
async fn additional_script_dir_overlapping_legacy_logs_and_wins_as_legacy() {
    let temp = LocalTempDir::new("ws_additional_overlapping_legacy_wins_as_legacy");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let legacy_path = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// override\n",
    );
    let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.additional_script_dirs = vec![legacy_dir.clone()];
        c.legacy_script_dirs = vec![legacy_dir];
    });

    backend.index_base_scripts().await;

    let snap = backend.snapshot();
    let base_docs = &snap.base_scripts_documents;
    assert!(
        base_docs.contains_key(base_url.as_str()),
        "legacy semantics must shadow the base script, not remove it from the base index"
    );
    assert!(
        !base_docs.contains_key(legacy_url.as_str()),
        "legacy file must not be loaded as a base overlay"
    );
    assert!(
        backend
            .snapshot()
            .suppressed_base_uris
            .contains(base_url.as_str()),
        "overlapping legacy dir must suppress the replaced base URI",
    );

    let ws_docs = &snap.workspace_documents;
    assert!(
        ws_docs.contains_key(legacy_url.as_str()),
        "legacy file must land in workspace_documents"
    );
}

#[tokio::test]
async fn watched_legacy_change_updates_workspace_incrementally() {
    let temp = LocalTempDir::new("ws_watched_legacy_incremental");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let legacy_path = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );
    let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir];
    });

    backend.index_base_scripts().await;
    let base_docs_before = backend.snapshot().base_scripts_documents.len();

    std::fs::write(&legacy_path, "class CR4Player {}\n// legacy edited\n")
        .expect("write legacy file");
    backend.apply_watched_file_events(vec![FileEvent {
        uri: legacy_url.clone(),
        typ: FileChangeType::CHANGED,
    }]);

    assert_eq!(
        backend.snapshot().base_scripts_documents.len(),
        base_docs_before,
        "a legacy file watch must not rebuild the entire base index"
    );
    assert!(
        backend
            .snapshot()
            .suppressed_base_uris
            .contains(base_url.as_str()),
        "override pairing must remain after an incremental legacy watch",
    );
    let ws_source = backend
        .snapshot()
        .workspace_documents
        .get(legacy_url.as_str())
        .expect("legacy file in workspace")
        .source
        .clone();
    assert!(
        ws_source.contains("// legacy edited"),
        "watched change must update workspace_documents"
    );
}
