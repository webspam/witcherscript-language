use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::router::Router;
use async_lsp::ClientSocket;
use lsp_types::{
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, FileChangeType, FileEvent,
    TextDocumentIdentifier, TextDocumentItem, Url,
};
use tokio::sync::mpsc;
use witcherscript_language::diagnostics::{
    collect_base_script_conflict_diagnostics, collect_duplicate_symbol_diagnostics,
};
use witcherscript_language::files::canonical_uri;

use crate::backend::{Backend, DocOp};
use crate::config::{Config, DiagnosticsScope};
use crate::indexing::{legacy_base_replacements, legacy_replaces_base};
use crate::watcher::event_touches_legacy_dir;

pub(super) struct LocalTempDir {
    path: PathBuf,
}

impl LocalTempDir {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("mkdir tempdir");
        Self { path }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LocalTempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
}

pub(super) fn make_backend() -> Backend {
    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let (doc_ops_tx, _doc_ops_rx) = mpsc::unbounded_channel();
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));
    Backend::new(client, config, doc_ops_tx)
}

pub(super) fn write_script(dir: &Path, rel: &str, contents: &str) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(&path, contents).expect("write script");
    path
}

fn make_game_dir(temp: &Path, rel: &str, contents: &str) -> (PathBuf, Url) {
    let game_dir = temp.join("game");
    let full_rel = Path::new("content")
        .join("content0")
        .join("scripts")
        .join(rel);
    let path = write_script(&game_dir, full_rel.to_str().unwrap(), contents);
    let url = Url::from_file_path(&path).expect("base path -> url");
    (game_dir, url)
}

async fn indexed_legacy_override(name: &str) -> (LocalTempDir, Backend, Url, Url) {
    let temp = LocalTempDir::new(name);
    let (game_dir, _base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let override_path = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );
    let new_path = write_script(&legacy_dir, "game/MyNewMod.ws", "class CMyNewMod {}\n");
    let override_url = Url::from_file_path(&override_path).expect("override path -> url");
    let new_url = Url::from_file_path(&new_path).expect("new path -> url");

    let backend = make_backend();
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir];
    backend.index_base_scripts().await;
    (temp, backend, override_url, new_url)
}

fn open_op(uri: &Url, text: &str) -> DocOp {
    DocOp::Open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "witcherscript".to_string(),
            version: 1,
            text: text.to_string(),
        },
    })
}

#[test]
fn legacy_replaces_base_matches_same_relpath() {
    assert!(legacy_replaces_base(
        "file:///game/content/content0/scripts/game/r4Player.ws",
        "file:///mod/legacy/game/r4Player.ws",
    ));
}

#[test]
fn legacy_replaces_base_requires_path_separator() {
    assert!(!legacy_replaces_base(
        "file:///game/content/content0/scripts/game/r4Player.ws",
        "file:///mod/legacy/Xgame/r4Player.ws",
    ));
}

#[test]
fn legacy_replaces_base_skips_base_without_scripts_segment() {
    assert!(!legacy_replaces_base(
        "file:///game/r4Player.ws",
        "file:///mod/legacy/r4Player.ws",
    ));
}

#[test]
fn legacy_replaces_base_basename_only_no_match() {
    assert!(!legacy_replaces_base(
        "file:///game/content/content0/scripts/game/r4Player.ws",
        "file:///mod/legacy/r4Player.ws",
    ));
}

#[test]
fn event_touches_legacy_dir_true_inside() {
    let temp = LocalTempDir::new("ws_event_touches_legacy_dir_true_inside");
    let file = temp.path().join("game").join("r4Player.ws");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "").unwrap();
    let event = FileEvent {
        uri: Url::from_file_path(&file).unwrap(),
        typ: FileChangeType::CHANGED,
    };
    assert!(event_touches_legacy_dir(
        &event,
        &[temp.path().to_path_buf()]
    ));
}

#[test]
fn event_touches_legacy_dir_false_outside() {
    let temp = LocalTempDir::new("ws_event_touches_legacy_dir_false_outside");
    let legacy = temp.path().join("legacy");
    std::fs::create_dir_all(&legacy).unwrap();
    let elsewhere = temp.path().join("workspace").join("foo.ws");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    std::fs::write(&elsewhere, "").unwrap();
    let event = FileEvent {
        uri: Url::from_file_path(&elsewhere).unwrap(),
        typ: FileChangeType::CHANGED,
    };
    assert!(!event_touches_legacy_dir(&event, &[legacy]));
}

#[test]
fn event_touches_legacy_dir_empty_dirs_returns_false() {
    let temp = LocalTempDir::new("ws_event_touches_legacy_dir_empty_dirs_returns_false");
    let file = temp.path().join("foo.ws");
    std::fs::write(&file, "").unwrap();
    let event = FileEvent {
        uri: Url::from_file_path(&file).unwrap(),
        typ: FileChangeType::CHANGED,
    };
    assert!(!event_touches_legacy_dir(&event, &[]));
}

#[tokio::test]
async fn matching_legacy_file_drops_base_and_lands_in_workspace() {
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
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir.clone()];

    backend.index_base_scripts().await;

    let base_docs = backend.base_scripts_documents.lock().await;
    assert!(
        !base_docs.contains_key(base_url.as_str()),
        "base script should be replaced; got keys {:?}",
        base_docs.keys().collect::<Vec<_>>()
    );

    let ws_docs = backend.workspace_documents.lock().await;
    assert!(
        ws_docs.contains_key(legacy_url.as_str()),
        "legacy file should be in workspace_documents; got keys {:?}",
        ws_docs.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn mod_shared_imports_override_drops_base_and_lands_in_workspace() {
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
    *backend.base_scripts_path.lock().await = Some(game_dir);

    backend.index_base_scripts().await;

    assert!(
        !backend
            .base_scripts_documents
            .lock()
            .await
            .contains_key(base_url.as_str()),
        "a modSharedImports replacement script must drop the base script it overrides"
    );
    assert!(
        backend
            .workspace_documents
            .lock()
            .await
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
    *backend.base_scripts_path.lock().await = Some(game_dir);
    backend.config.store(Arc::new(Config {
        auto_load_mod_shared_imports: false,
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));

    backend.index_base_scripts().await;

    assert!(
        backend
            .base_scripts_documents
            .lock()
            .await
            .contains_key(base_url.as_str()),
        "with auto-load off the base script must stay in the base index"
    );
    assert!(
        !backend
            .workspace_documents
            .lock()
            .await
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
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

    backend.index_base_scripts().await;
    assert!(
        backend
            .workspace_documents
            .lock()
            .await
            .contains_key(legacy_url.as_str()),
        "legacy file should be indexed into the workspace first"
    );

    std::fs::remove_file(&legacy_path).expect("remove legacy file");
    backend.index_base_scripts().await;

    assert!(
        !backend
            .workspace_documents
            .lock()
            .await
            .contains_key(legacy_url.as_str()),
        "a deleted legacy file must not linger in workspace_documents"
    );
    assert!(
        backend.legacy_indexed_uris.lock().await.is_empty(),
        "tracked legacy URIs must be cleared once the file is gone"
    );
    assert!(
        backend
            .base_scripts_documents
            .lock()
            .await
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
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir.clone()];

    backend.index_base_scripts().await;

    let base_docs = backend.base_scripts_documents.lock().await;
    assert!(
        base_docs.contains_key(base_url.as_str()),
        "unmatched legacy file must not remove the base script"
    );

    let ws_docs = backend.workspace_documents.lock().await;
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
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

    backend.index_base_scripts().await;

    let ws = backend.workspace_index.lock().await;
    let base = backend.base_scripts_index.lock().await;
    let legacy_dirs = backend.legacy_script_dirs.lock().await.clone();
    let diagnostics = collect_base_script_conflict_diagnostics(&ws, &base, &legacy_dirs);
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
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir];
    backend.index_base_scripts().await;

    backend
        .update_open_document(base_url.clone(), "class CR4Player {}\n".to_string())
        .await;

    let ws = backend.workspace_index.lock().await;
    let base = backend.base_scripts_index.lock().await;
    let legacy_dirs = backend.legacy_script_dirs.lock().await.clone();

    assert!(
        collect_duplicate_symbol_diagnostics(&ws).is_empty(),
        "opening the overridden base script must not create a workspace duplicate",
    );
    assert!(
        collect_base_script_conflict_diagnostics(&ws, &base, &legacy_dirs).is_empty(),
        "the legacy override must not be flagged once both files are loaded",
    );
    assert!(
        base.documents().any(|(uri, _)| uri == base_url.as_str()),
        "the opened base script should be indexed as a base script",
    );
}

#[tokio::test]
async fn reindexing_keeps_an_open_legacy_file_indexed() {
    let temp = LocalTempDir::new("ws_reindex_keeps_open_legacy");
    let (game_dir, _base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let legacy_path = write_script(&legacy_dir, "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

    let backend = make_backend();
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

    backend.index_base_scripts().await;
    backend
        .update_open_document(legacy_url.clone(), "class CR4Player {}\n".to_string())
        .await;
    backend.index_base_scripts().await;

    assert!(
        backend
            .workspace_index
            .lock()
            .await
            .documents()
            .any(|(uri, _)| uri == legacy_url.as_str()),
        "an open legacy file must survive a re-index",
    );
}

#[tokio::test]
async fn reindexing_keeps_an_open_overridden_base_script_indexed() {
    let temp = LocalTempDir::new("ws_reindex_keeps_open_overridden_base");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let _ = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );

    let backend = make_backend();
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

    backend.index_base_scripts().await;
    backend
        .update_open_document(base_url.clone(), "class CR4Player {}\n".to_string())
        .await;
    backend.index_base_scripts().await;

    assert!(
        backend
            .base_scripts_index
            .lock()
            .await
            .documents()
            .any(|(uri, _)| uri == base_url.as_str()),
        "an open, legacy-overridden base script must survive a re-index",
    );
}

#[test]
fn legacy_base_replacements_maps_only_real_overrides() {
    struct Case {
        name: &'static str,
        base: &'static [&'static str],
        legacy: &'static [&'static str],
        expect_skip: &'static [&'static str],
        expect_map: &'static [(&'static str, &'static str)],
    }
    let cases = [
        Case {
            name: "legacy file at the same game-relative path replaces the base script",
            base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
            legacy: &["file:///mod/legacy/game/r4Player.ws"],
            expect_skip: &["file:///game/content/content0/scripts/game/r4Player.ws"],
            expect_map: &[("file:///mod/legacy/game/r4Player.ws", "game/r4Player.ws")],
        },
        Case {
            name: "brand-new script in a legacy folder replaces nothing",
            base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
            legacy: &["file:///mod/legacy/game/MyNewMod.ws"],
            expect_skip: &[],
            expect_map: &[],
        },
        Case {
            name: "same basename but a different relative path replaces nothing",
            base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
            legacy: &["file:///mod/legacy/other/r4Player.ws"],
            expect_skip: &[],
            expect_map: &[],
        },
    ];
    for c in cases {
        let base: Vec<String> = c.base.iter().map(|s| s.to_string()).collect();
        let legacy: Vec<String> = c.legacy.iter().map(|s| s.to_string()).collect();
        let (skip, map) = legacy_base_replacements(&base, &legacy);
        let expect_skip: HashSet<String> = c.expect_skip.iter().map(|s| s.to_string()).collect();
        let expect_map: HashMap<String, String> = c
            .expect_map
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(skip, expect_skip, "case '{}': skip set mismatch", c.name);
        assert_eq!(
            map, expect_map,
            "case '{}': replacement map mismatch",
            c.name
        );
    }
}

#[tokio::test]
async fn index_base_scripts_records_only_real_legacy_overrides() {
    let (_temp, backend, override_url, new_url) =
        indexed_legacy_override("ws_legacy_replacements_map").await;

    let map = backend.legacy_replacements.lock().await;
    let override_key = canonical_uri(&override_url).expect("canonical override uri");
    assert_eq!(
        map.get(&override_key).map(String::as_str),
        Some("game/r4Player.ws"),
        "a legacy file overriding a base script must record the replaced path",
    );
    let new_key = canonical_uri(&new_url).expect("canonical new uri");
    assert!(
        !map.contains_key(&new_key),
        "a brand-new script in a legacy folder must not be recorded as a replacement",
    );
}

#[tokio::test]
async fn opening_a_legacy_override_marks_it_as_replacing_a_base_script() {
    let (_temp, backend, override_url, new_url) =
        indexed_legacy_override("ws_legacy_status_open").await;

    backend
        .dispatch_doc_op(open_op(&override_url, "class CR4Player {}\n"))
        .await;
    backend
        .dispatch_doc_op(open_op(&new_url, "class CMyNewMod {}\n"))
        .await;

    let sent = backend.sent_legacy_status.lock().await;
    let override_status = sent
        .get(&override_url)
        .expect("status sent for the override file");
    assert!(
        override_status.replaces_base_script,
        "an open legacy override must be reported as replacing a base script",
    );
    assert_eq!(
        override_status.replaced_script_path.as_deref(),
        Some("game/r4Player.ws"),
    );
    let new_status = sent.get(&new_url).expect("status sent for the new file");
    assert!(
        !new_status.replaces_base_script,
        "a brand-new script in a legacy folder must not be reported as replacing a base script",
    );
}

#[tokio::test]
async fn closing_a_legacy_override_keeps_its_status_dedup_entry() {
    let (_temp, backend, override_url, _new_url) =
        indexed_legacy_override("ws_legacy_status_close").await;
    backend
        .dispatch_doc_op(open_op(&override_url, "class CR4Player {}\n"))
        .await;

    backend
        .dispatch_doc_op(DocOp::Close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: override_url.clone(),
            },
        }))
        .await;

    assert!(
        backend
            .sent_legacy_status
            .lock()
            .await
            .contains_key(&override_url),
        "closing a file must keep its status dedup entry, or an unrelated edit \
         would re-push a notification for the closed file",
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
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.additional_script_dirs.lock().await = vec![legacy_dir.clone()];
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

    backend.index_base_scripts().await;

    let base_docs = backend.base_scripts_documents.lock().await;
    assert!(
        !base_docs.contains_key(base_url.as_str()),
        "legacy semantics must win when the same dir appears in both lists"
    );
    assert!(
        !base_docs.contains_key(legacy_url.as_str()),
        "legacy file must not be loaded as a base overlay"
    );

    let ws_docs = backend.workspace_documents.lock().await;
    assert!(
        ws_docs.contains_key(legacy_url.as_str()),
        "legacy file must land in workspace_documents"
    );
}
