use std::path::Path;

use lsp_types::{DeleteFilesParams, FileDelete, FileRename, RenameFilesParams, Url};

use super::legacy_helpers::{make_backend, write_script, LocalTempDir};
use crate::backend::{Backend, DocOp};
use crate::convert::{deleted_files_to_watched, renamed_files_to_watched};

async fn index_dir(backend: &Backend, dir: &Path) {
    *backend.workspace_roots.lock().await = vec![dir.to_path_buf()];
    backend.index_workspace().await;
}

fn delete_op(url: &Url) -> DocOp {
    DocOp::WatchedFiles(deleted_files_to_watched(DeleteFilesParams {
        files: vec![FileDelete {
            uri: url.to_string(),
        }],
    }))
}

fn rename_op(old: &Url, new: &Url) -> DocOp {
    DocOp::WatchedFiles(renamed_files_to_watched(RenameFilesParams {
        files: vec![FileRename {
            old_uri: old.to_string(),
            new_uri: new.to_string(),
        }],
    }))
}

#[tokio::test]
async fn deleting_a_file_removes_it_from_the_index() {
    let temp = LocalTempDir::new("ws_fileop_delete");
    let path = write_script(temp.path(), "Helper.ws", "class CHelper {}\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend();
    index_dir(&backend, temp.path()).await;
    assert!(
        backend
            .workspace_documents
            .lock()
            .await
            .contains_key(url.as_str()),
        "file must be indexed before deletion can be exercised",
    );

    backend.dispatch_doc_op(delete_op(&url)).await;

    assert!(
        !backend
            .workspace_documents
            .lock()
            .await
            .contains_key(url.as_str()),
        "a deleted file must be dropped from the workspace index",
    );
}

#[tokio::test]
async fn renaming_a_file_moves_it_in_the_index() {
    let temp = LocalTempDir::new("ws_fileop_rename");
    let old_path = write_script(temp.path(), "Old.ws", "class COld {}\n");
    let old_url = Url::from_file_path(&old_path).expect("old path -> url");
    let new_path = temp.path().join("New.ws");
    let new_url = Url::from_file_path(&new_path).expect("new path -> url");

    let backend = make_backend();
    index_dir(&backend, temp.path()).await;

    std::fs::rename(&old_path, &new_path).expect("rename on disk");
    backend.dispatch_doc_op(rename_op(&old_url, &new_url)).await;

    let docs = backend.workspace_documents.lock().await;
    assert!(
        !docs.contains_key(old_url.as_str()),
        "the old name must be dropped after a rename",
    );
    assert!(
        docs.contains_key(new_url.as_str()),
        "the new name must be indexed after a rename",
    );
}

#[tokio::test]
async fn repeated_delete_is_idempotent() {
    let temp = LocalTempDir::new("ws_fileop_delete_twice");
    let path = write_script(temp.path(), "Helper.ws", "class CHelper {}\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend();
    index_dir(&backend, temp.path()).await;

    backend.dispatch_doc_op(delete_op(&url)).await;
    backend.dispatch_doc_op(delete_op(&url)).await;

    assert!(
        !backend
            .workspace_documents
            .lock()
            .await
            .contains_key(url.as_str()),
        "a duplicated delete (OS watcher + fileOperations) must stay a harmless no-op",
    );
}

#[tokio::test]
async fn deleting_an_open_file_keeps_the_editor_buffer() {
    let temp = LocalTempDir::new("ws_fileop_delete_open");
    let path = write_script(temp.path(), "Open.ws", "class COpen {}\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend();
    index_dir(&backend, temp.path()).await;
    backend
        .update_open_document(url.clone(), "class COpen {}\n".to_string())
        .await;

    backend.dispatch_doc_op(delete_op(&url)).await;

    assert!(
        backend.documents.lock().await.contains_key(&url),
        "a file-operation event must not evict a file that is open in the editor",
    );
}

#[tokio::test]
async fn renaming_an_open_file_keeps_the_editor_buffer() {
    let temp = LocalTempDir::new("ws_fileop_rename_open");
    let old_path = write_script(temp.path(), "Old.ws", "class COld {}\n");
    let old_url = Url::from_file_path(&old_path).expect("old path -> url");
    let new_path = temp.path().join("New.ws");
    let new_url = Url::from_file_path(&new_path).expect("new path -> url");

    let backend = make_backend();
    index_dir(&backend, temp.path()).await;
    backend
        .update_open_document(old_url.clone(), "class COld {}\n".to_string())
        .await;

    std::fs::rename(&old_path, &new_path).expect("rename on disk");
    backend.dispatch_doc_op(rename_op(&old_url, &new_url)).await;

    assert!(
        backend.documents.lock().await.contains_key(&old_url),
        "renaming an open file must not evict its editor buffer",
    );
}
