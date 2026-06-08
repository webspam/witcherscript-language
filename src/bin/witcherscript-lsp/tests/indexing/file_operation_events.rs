use std::path::Path;

use lsp_types::{
    DeleteFilesParams, DidChangeWatchedFilesParams, FileDelete, FileRename, RenameFilesParams, Url,
};

use super::legacy_helpers::{LocalTempDir, make_backend, write_script};
use crate::backend::Backend;
use crate::convert::{deleted_files_to_watched, renamed_files_to_watched};

async fn index_dir(backend: &Backend, dir: &Path) {
    backend.set_workspace_roots(vec![dir.to_path_buf()]);
    backend.index_workspace().await;
}

fn delete_params(url: &Url) -> DidChangeWatchedFilesParams {
    deleted_files_to_watched(DeleteFilesParams {
        files: vec![FileDelete {
            uri: url.to_string(),
        }],
    })
}

fn rename_params(old: &Url, new: &Url) -> DidChangeWatchedFilesParams {
    renamed_files_to_watched(RenameFilesParams {
        files: vec![FileRename {
            old_uri: old.to_string(),
            new_uri: new.to_string(),
        }],
    })
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
            .snapshot()
            .workspace_documents
            .contains_key(url.as_str()),
        "file must be indexed before deletion can be exercised",
    );

    backend._did_change_watched_files(delete_params(&url));

    assert!(
        !backend
            .snapshot()
            .workspace_documents
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
    backend._did_change_watched_files(rename_params(&old_url, &new_url));

    let snap = backend.snapshot();
    let docs = &snap.workspace_documents;
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

    backend._did_change_watched_files(delete_params(&url));
    backend._did_change_watched_files(delete_params(&url));

    assert!(
        !backend
            .snapshot()
            .workspace_documents
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
    backend.update_open_document(url.clone(), "class COpen {}\n".to_string());

    backend._did_change_watched_files(delete_params(&url));

    assert!(
        backend.snapshot().documents.contains_key(&url),
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
    backend.update_open_document(old_url.clone(), "class COld {}\n".to_string());

    std::fs::rename(&old_path, &new_path).expect("rename on disk");
    backend._did_change_watched_files(rename_params(&old_url, &new_url));

    assert!(
        backend.snapshot().documents.contains_key(&old_url),
        "renaming an open file must not evict its editor buffer",
    );
}
