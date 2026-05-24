use lsp_types::{
    DidChangeWorkspaceFoldersParams, Url, WorkspaceFolder, WorkspaceFoldersChangeEvent,
};

use super::legacy_routing::{make_backend, write_script, LocalTempDir};
use crate::backend::DocOp;

fn folders(uris: &[&Url]) -> Vec<WorkspaceFolder> {
    uris.iter()
        .map(|uri| WorkspaceFolder {
            uri: (*uri).clone(),
            name: "folder".to_string(),
        })
        .collect()
}

fn folder_change(added: &[&Url], removed: &[&Url]) -> DocOp {
    DocOp::WorkspaceFolders(DidChangeWorkspaceFoldersParams {
        event: WorkspaceFoldersChangeEvent {
            added: folders(added),
            removed: folders(removed),
        },
    })
}

#[tokio::test]
async fn adding_a_folder_indexes_its_scripts() {
    let temp = LocalTempDir::new("ws_added_folder_indexes");
    let script = write_script(temp.path(), "Helper.ws", "class CHelper {}\n");
    let script_url = Url::from_file_path(&script).expect("script path -> url");
    let folder_url = Url::from_file_path(temp.path()).expect("folder path -> url");

    let backend = make_backend();
    backend
        .dispatch_doc_op(folder_change(&[&folder_url], &[]))
        .await;

    assert!(
        backend
            .workspace_documents
            .lock()
            .await
            .contains_key(script_url.as_str()),
        "a script in a newly added workspace folder must be indexed",
    );
}

#[tokio::test]
async fn removing_a_folder_drops_its_scripts() {
    let temp = LocalTempDir::new("ws_removed_folder_drops");
    let script = write_script(temp.path(), "Helper.ws", "class CHelper {}\n");
    let script_url = Url::from_file_path(&script).expect("script path -> url");
    let folder_url = Url::from_file_path(temp.path()).expect("folder path -> url");

    let backend = make_backend();
    backend
        .dispatch_doc_op(folder_change(&[&folder_url], &[]))
        .await;
    assert!(
        backend
            .workspace_documents
            .lock()
            .await
            .contains_key(script_url.as_str()),
        "folder must be indexed before removal can be exercised",
    );

    backend
        .dispatch_doc_op(folder_change(&[], &[&folder_url]))
        .await;
    assert!(
        !backend
            .workspace_documents
            .lock()
            .await
            .contains_key(script_url.as_str()),
        "a script in a removed workspace folder must be dropped from the index",
    );
}
