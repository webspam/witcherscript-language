use std::path::PathBuf;

use lsp_types::{
    CreateFilesParams, DeleteFilesParams, DidChangeWatchedFilesParams, FileChangeType, FileEvent,
    InitializeParams, RenameFilesParams, Url,
};
use witcherscript_language::files::is_witcherscript_file;

#[allow(deprecated)]
pub(crate) fn workspace_roots(params: InitializeParams) -> Vec<PathBuf> {
    if let Some(folders) = params.workspace_folders {
        return folders
            .into_iter()
            .filter_map(|folder| folder.uri.to_file_path().ok())
            .collect();
    }

    params
        .root_uri
        .and_then(|uri| uri.to_file_path().ok())
        .filter(|path| path.is_dir() || is_witcherscript_file(path))
        .into_iter()
        .collect()
}

// File-operation notifications reuse the watched-file pipeline rather than a parallel one.
pub(crate) fn created_files_to_watched(params: CreateFilesParams) -> DidChangeWatchedFilesParams {
    DidChangeWatchedFilesParams {
        changes: params
            .files
            .into_iter()
            .filter_map(|file| Url::parse(&file.uri).ok())
            .map(|uri| FileEvent::new(uri, FileChangeType::CREATED))
            .collect(),
    }
}

pub(crate) fn deleted_files_to_watched(params: DeleteFilesParams) -> DidChangeWatchedFilesParams {
    DidChangeWatchedFilesParams {
        changes: params
            .files
            .into_iter()
            .filter_map(|file| Url::parse(&file.uri).ok())
            .map(|uri| FileEvent::new(uri, FileChangeType::DELETED))
            .collect(),
    }
}

pub(crate) fn renamed_files_to_watched(params: RenameFilesParams) -> DidChangeWatchedFilesParams {
    let mut changes = Vec::new();
    for file in params.files {
        if let Ok(old_uri) = Url::parse(&file.old_uri) {
            changes.push(FileEvent::new(old_uri, FileChangeType::DELETED));
        }
        if let Ok(new_uri) = Url::parse(&file.new_uri) {
            changes.push(FileEvent::new(new_uri, FileChangeType::CREATED));
        }
    }
    DidChangeWatchedFilesParams { changes }
}
