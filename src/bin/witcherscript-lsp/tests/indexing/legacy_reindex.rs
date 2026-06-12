use std::sync::atomic::Ordering;

use lsp_types::{DidCloseTextDocumentParams, TextDocumentIdentifier, Url};

use witcherscript_language::files::canonical_uri;

use super::legacy_helpers::{indexed_legacy_override, make_game_dir, write_script};
use crate::tests::support::{LocalTempDir, make_backend, open_params};

#[tokio::test]
async fn reindexing_keeps_an_open_legacy_file_indexed() {
    let temp = LocalTempDir::new("ws_reindex_keeps_open_legacy");
    let (game_dir, _base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let legacy_path = write_script(&legacy_dir, "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir];
    });

    backend.index_base_scripts().await;
    backend.update_open_document(legacy_url.clone(), "class CR4Player {}\n".to_string());
    backend.index_base_scripts().await;

    assert!(
        backend
            .snapshot()
            .workspace_index
            .documents()
            .any(|(uri, _)| uri == legacy_url.as_str()),
        "an open legacy file must survive a re-index",
    );
}

#[tokio::test]
async fn refresh_override_maps_keeps_open_legacy_pairing() {
    let temp = LocalTempDir::new("ws_refresh_open_legacy_pairing");
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
    backend.update_open_document(
        legacy_url.clone(),
        "class CR4Player {}\n// legacy\n".to_string(),
    );
    backend.publish_compilation(|builder| {
        builder
            .workspace_documents_mut()
            .remove(legacy_url.as_str());
    });

    backend.refresh_legacy_override_maps();

    assert!(
        backend
            .snapshot()
            .suppressed_base_uris
            .contains(base_url.as_str()),
        "refresh must pair an open legacy override using workspace_index, not workspace_documents",
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
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir];
    });

    backend.index_base_scripts().await;
    backend.update_open_document(base_url.clone(), "class CR4Player {}\n".to_string());
    backend.index_base_scripts().await;

    assert!(
        backend
            .snapshot()
            .base_scripts_index
            .documents()
            .any(|(uri, _)| uri == base_url.as_str()),
        "an open, legacy-overridden base script must survive a re-index",
    );
}

#[tokio::test]
async fn index_base_scripts_records_only_real_legacy_overrides() {
    let (_temp, backend, override_url, new_url) =
        indexed_legacy_override("ws_legacy_replacements_map").await;

    let map = backend.legacy_replacements.load();
    let override_key = canonical_uri(&override_url);
    assert_eq!(
        map.get(&override_key).map(String::as_str),
        Some("game/r4Player.ws"),
        "a legacy file overriding a base script must record the replaced path",
    );
    let new_key = canonical_uri(&new_url);
    assert!(
        !map.contains_key(&new_key),
        "a brand-new script in a legacy folder must not be recorded as a replacement",
    );
}

#[tokio::test]
async fn did_open_refreshes_legacy_override_maps_for_open_first_override() {
    let temp = LocalTempDir::new("ws_did_open_refresh_legacy_maps");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir.clone()];
    });
    backend.index_base_scripts().await;

    let legacy_path = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );
    let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");
    backend._did_open(open_params(&legacy_url, "class CR4Player {}\n// legacy\n"));

    assert!(
        backend
            .snapshot()
            .suppressed_base_uris
            .contains(base_url.as_str()),
        "did_open must refresh suppress maps when the override was not indexed before open",
    );
}

#[tokio::test]
async fn reopening_unchanged_legacy_file_does_not_bump_state_version() {
    let (_temp, backend, override_url, _new_url) =
        indexed_legacy_override("ws_reopen_legacy_no_bump").await;
    let text = "class CR4Player {}\n// legacy\n";

    backend._did_open(open_params(&override_url, text));
    let version_after_first = backend.state_version.load(Ordering::Acquire);
    backend._did_open(open_params(&override_url, text));

    assert_eq!(
        backend.state_version.load(Ordering::Acquire),
        version_after_first,
        "re-opening a byte-identical legacy file must not bump state_version",
    );
}

#[tokio::test]
async fn opening_a_legacy_override_marks_it_as_replacing_a_base_script() {
    let (_temp, backend, override_url, new_url) =
        indexed_legacy_override("ws_legacy_status_open").await;

    backend._did_open(open_params(&override_url, "class CR4Player {}\n"));
    backend._did_open(open_params(&new_url, "class CMyNewMod {}\n"));

    let sent = backend.sent_legacy_status.lock();
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
    backend._did_open(open_params(&override_url, "class CR4Player {}\n"));

    backend._did_close(DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier {
            uri: override_url.clone(),
        },
    });

    assert!(
        backend
            .sent_legacy_status
            .lock()
            .contains_key(&override_url),
        "closing a file must keep its status dedup entry, or an unrelated edit \
         would re-push a notification for the closed file",
    );
}
