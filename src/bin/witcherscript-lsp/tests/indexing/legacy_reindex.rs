use lsp_types::{DidCloseTextDocumentParams, TextDocumentIdentifier, Url};

use witcherscript_language::files::canonical_uri;

use crate::backend::DocOp;

use super::legacy_helpers::{
    indexed_legacy_override, make_backend, make_game_dir, open_op, write_script, LocalTempDir,
};

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
