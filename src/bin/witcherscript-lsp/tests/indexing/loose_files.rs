use lsp_types::{
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, Position, TextDocumentIdentifier,
    TextDocumentItem, Url,
};
use witcherscript_language::diagnostics::collect_duplicate_symbol_diagnostics;

use super::legacy_helpers::{make_backend, write_script, LocalTempDir};
use crate::file_scope::FileScope;

fn open_params(uri: &Url, text: &str) -> DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "witcherscript".to_string(),
            version: 1,
            text: text.to_string(),
        },
    }
}

fn close_params(uri: &Url) -> DidCloseTextDocumentParams {
    DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    }
}

#[tokio::test]
async fn loose_file_lands_in_loose_index_not_workspace() {
    let temp = LocalTempDir::new("ws_loose_lands_in_loose_index");
    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let loose_path = write_script(temp.path(), "outside/Loose.ws", "class CLoose {}\n");
    let loose_url = Url::from_file_path(&loose_path).unwrap();

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![project_dir];
    backend.update_open_document(loose_url.clone(), "class CLoose {}\n".to_string());

    assert!(
        backend
            .snapshot()
            .loose_index
            .documents()
            .any(|(u, _)| u == loose_url.as_str()),
        "a file outside every workspace root must land in loose_index",
    );
    assert!(
        !backend
            .snapshot()
            .workspace_index
            .documents()
            .any(|(u, _)| u == loose_url.as_str()),
        "a loose file must not pollute workspace_index",
    );
}

#[tokio::test]
async fn single_file_lands_in_loose_index() {
    let temp = LocalTempDir::new("ws_single_file_loose");
    let path = write_script(temp.path(), "Solo.ws", "class CSolo {}\n");
    let url = Url::from_file_path(&path).unwrap();

    let backend = make_backend();
    backend.update_open_document(url.clone(), "class CSolo {}\n".to_string());

    assert!(
        backend
            .snapshot()
            .loose_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "with no workspace folder open, an opened file must land in loose_index",
    );
}

#[tokio::test]
async fn closing_a_loose_file_drops_it_from_loose_index_and_documents() {
    let temp = LocalTempDir::new("ws_close_loose_drops");
    let path = write_script(temp.path(), "Solo.ws", "class CSolo {}\n");
    let url = Url::from_file_path(&path).unwrap();

    let backend = make_backend();
    backend._did_open(open_params(&url, "class CSolo {}\n"));
    backend._did_close(close_params(&url));

    assert!(
        !backend
            .snapshot()
            .loose_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "closing a loose file must drop it from loose_index",
    );
    assert!(
        !backend.snapshot().documents.contains_key(&url),
        "closing a loose file must drop it from the open documents map",
    );
}

#[tokio::test]
async fn closing_a_project_file_keeps_it_indexed() {
    let temp = LocalTempDir::new("ws_close_project_keeps");
    let path = write_script(temp.path(), "Helper.ws", "class CHelper {}\n");
    let url = Url::from_file_path(&path).unwrap();

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    backend._did_open(open_params(&url, "class CHelper {}\n"));
    backend._did_close(close_params(&url));

    assert!(
        backend
            .snapshot()
            .workspace_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "closing an in-project file must leave it indexed",
    );
}

#[tokio::test]
async fn scope_change_between_open_and_change_does_not_leak() {
    let temp = LocalTempDir::new("ws_scope_change_no_leak");
    let path = write_script(temp.path(), "File.ws", "class CFile {}\n");
    let url = Url::from_file_path(&path).unwrap();

    let backend = make_backend();
    backend.update_open_document(url.clone(), "class CFile {}\n".to_string());
    assert!(
        backend
            .snapshot()
            .loose_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "the file should start in loose_index",
    );

    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    backend.update_open_document(url.clone(), "class CFile {}\n// edit\n".to_string());

    assert!(
        !backend
            .snapshot()
            .loose_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "a stale loose_index entry must not survive the scope change",
    );
    assert!(
        backend
            .snapshot()
            .workspace_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "the file must move into workspace_index once it becomes in-project",
    );
}

#[tokio::test]
async fn two_loose_files_share_one_compilation() {
    let temp = LocalTempDir::new("ws_two_loose_share");
    let alpha_path = write_script(temp.path(), "Alpha.ws", "class CAlpha {}\n");
    let alpha_url = Url::from_file_path(&alpha_path).unwrap();
    let beta_path = write_script(temp.path(), "Beta.ws", "x");
    let beta_url = Url::from_file_path(&beta_path).unwrap();

    let backend = make_backend();
    backend.update_open_document(alpha_url.clone(), "class CAlpha {}\n".to_string());
    backend.update_open_document(
        beta_url.clone(),
        "function F()\n{\n\tvar x : CAlpha;\n}\n".to_string(),
    );

    let def = backend.resolve_at(&beta_url, Position::new(2, 11));
    assert!(
        def.is_some(),
        "a loose file must resolve a type declared in another open loose file",
    );
}

#[tokio::test]
async fn loose_file_does_not_resolve_project_symbols() {
    let temp = LocalTempDir::new("ws_loose_isolation_project");
    let project_dir = temp.path().join("project");
    write_script(&project_dir, "Foo.ws", "class CFoo {}\n");

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![project_dir];
    backend.index_workspace().await;

    let loose_path = write_script(temp.path(), "outside/Loose.ws", "x");
    let loose_url = Url::from_file_path(&loose_path).unwrap();
    backend.update_open_document(
        loose_url.clone(),
        "function F()\n{\n\tvar x : CFoo;\n}\n".to_string(),
    );

    let def = backend.resolve_at(&loose_url, Position::new(2, 11));
    assert!(
        def.is_none(),
        "a loose file must not resolve a type from the isolated workspace project",
    );
}

#[tokio::test]
async fn loose_and_project_file_with_same_class_do_not_conflict() {
    let temp = LocalTempDir::new("ws_loose_project_same_name");
    let project_dir = temp.path().join("project");
    write_script(&project_dir, "Same.ws", "class CSame {}\n");

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![project_dir];
    backend.index_workspace().await;

    let loose_path = write_script(temp.path(), "outside/Same.ws", "class CSame {}\n");
    let loose_url = Url::from_file_path(&loose_path).unwrap();
    backend.update_open_document(loose_url.clone(), "class CSame {}\n".to_string());

    let snap = backend.snapshot();
    assert!(
        collect_duplicate_symbol_diagnostics(snap.workspace_index.as_ref()).is_empty(),
        "a loose file must not collide with an identically named project class",
    );
    assert!(
        collect_duplicate_symbol_diagnostics(snap.loose_index.as_ref()).is_empty(),
        "an isolated loose file must not be flagged against the project",
    );
}

#[tokio::test]
async fn opening_a_loose_file_sends_file_scope_status() {
    let temp = LocalTempDir::new("ws_loose_status_open");
    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let loose_path = write_script(temp.path(), "outside/Loose.ws", "class CLoose {}\n");
    let loose_url = Url::from_file_path(&loose_path).unwrap();

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![project_dir];
    backend._did_open(open_params(&loose_url, "class CLoose {}\n"));

    let sent = backend.sent_file_scope_status.lock();
    let status = sent
        .get(&loose_url)
        .expect("a file scope status must be sent for the opened loose file");
    assert_eq!(
        status.scope,
        FileScope::OutOfScope,
        "an out-of-workspace file must be reported as out of scope",
    );
}

#[tokio::test]
async fn adding_a_workspace_folder_reroutes_open_loose_files() {
    use lsp_types::{
        DidChangeWorkspaceFoldersParams, WorkspaceFolder, WorkspaceFoldersChangeEvent,
    };

    let temp = LocalTempDir::new("ws_reindex_open_on_folder_add");
    let path = write_script(temp.path(), "File.ws", "class CFile {}\n");
    let url = Url::from_file_path(&path).unwrap();

    let backend = make_backend();
    backend._did_open(open_params(&url, "class CFile {}\n"));
    assert!(
        backend
            .snapshot()
            .loose_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "the file must start in loose_index (no workspace open yet)",
    );

    let folder_url = Url::from_file_path(temp.path()).unwrap();
    backend
        ._did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: vec![WorkspaceFolder {
                    uri: folder_url,
                    name: "folder".to_string(),
                }],
                removed: vec![],
            },
        })
        .await;

    assert!(
        !backend
            .snapshot()
            .loose_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "adding the containing folder must drop the file from loose_index immediately, not on next keystroke",
    );
    assert!(
        backend
            .snapshot()
            .workspace_index
            .documents()
            .any(|(u, _)| u == url.as_str()),
        "the file must land in workspace_index without an edit",
    );
}

#[tokio::test]
async fn closing_a_loose_file_clears_its_file_scope_status_dedup_entry() {
    let temp = LocalTempDir::new("ws_loose_status_close");
    let path = write_script(temp.path(), "Solo.ws", "class CSolo {}\n");
    let url = Url::from_file_path(&path).unwrap();

    let backend = make_backend();
    backend._did_open(open_params(&url, "class CSolo {}\n"));
    backend._did_close(close_params(&url));

    assert!(
        !backend.sent_file_scope_status.lock().contains_key(&url),
        "closing a loose file must clear its status dedup entry so a reopen re-pushes",
    );
}
