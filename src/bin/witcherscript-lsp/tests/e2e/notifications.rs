use lsp_types::notification::{
    DidChangeWorkspaceFolders, DidCreateFiles, DidDeleteFiles, DidRenameFiles,
};
use lsp_types::{
    CreateFilesParams, DeleteFilesParams, DidChangeWorkspaceFoldersParams, FileCreate, FileDelete,
    FileRename, OneOf, RenameFilesParams, Url, WorkspaceFoldersChangeEvent,
};

use super::harness::LspClient;

#[tokio::test]
async fn did_save_does_not_crash_server() {
    let uri: Url = "file:///save.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;
    let diags = client.pull_diagnostics(&uri).await;
    assert!(!diags.is_empty(), "broken source should report diagnostics");

    client.did_save(&uri).await;
    client.did_save(&uri).await;

    client.change_full(&uri, 2, "class Foo {}\n").await;
    let diags = client.pull_diagnostics(&uri).await;
    assert!(
        diags.is_empty(),
        "server should still respond after didSave, got {diags:?}"
    );
}

#[tokio::test]
async fn advertises_file_operation_capabilities() {
    let client = LspClient::spawn().await;
    let workspace = client
        .server_capabilities()
        .workspace
        .as_ref()
        .expect("workspace capabilities advertised");

    let file_ops = workspace
        .file_operations
        .as_ref()
        .expect("file_operations advertised");
    assert!(file_ops.did_create.is_some(), "didCreateFiles registered");
    assert!(file_ops.did_rename.is_some(), "didRenameFiles registered");
    assert!(file_ops.did_delete.is_some(), "didDeleteFiles registered");

    let folders = workspace
        .workspace_folders
        .as_ref()
        .expect("workspace_folders advertised");
    assert!(
        matches!(folders.change_notifications, Some(OneOf::Left(true))),
        "workspace folder change notifications must be enabled"
    );
}

#[tokio::test]
async fn file_operation_notifications_do_not_crash_server() {
    let uri: Url = "file:///fileops.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;
    let diags = client.pull_diagnostics(&uri).await;
    assert!(!diags.is_empty(), "broken source should report diagnostics");

    client
        .notify::<DidCreateFiles>(CreateFilesParams {
            files: vec![FileCreate {
                uri: "file:///created.ws".to_string(),
            }],
        })
        .await;
    client
        .notify::<DidRenameFiles>(RenameFilesParams {
            files: vec![FileRename {
                old_uri: "file:///created.ws".to_string(),
                new_uri: "file:///renamed.ws".to_string(),
            }],
        })
        .await;
    client
        .notify::<DidDeleteFiles>(DeleteFilesParams {
            files: vec![FileDelete {
                uri: "file:///renamed.ws".to_string(),
            }],
        })
        .await;
    client
        .notify::<DidChangeWorkspaceFolders>(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: Vec::new(),
                removed: Vec::new(),
            },
        })
        .await;

    client.change_full(&uri, 2, "class Foo {}\n").await;
    let diags = client.pull_diagnostics(&uri).await;
    assert!(
        diags.is_empty(),
        "server should still respond after file-operation notifications, got {diags:?}"
    );
}
