use super::*;
use lsp_types::{FileCreate, FileDelete, FileRename};

#[test]
fn created_files_become_created_events() {
    let changes = created_files_to_watched(CreateFilesParams {
        files: vec![
            FileCreate {
                uri: "file:///a.ws".to_string(),
            },
            FileCreate {
                uri: "file:///b.ws".to_string(),
            },
        ],
    })
    .changes;
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|e| e.typ == FileChangeType::CREATED));
    assert_eq!(changes[0].uri.as_str(), "file:///a.ws");
    assert_eq!(changes[1].uri.as_str(), "file:///b.ws");
}

#[test]
fn deleted_files_become_deleted_events() {
    let changes = deleted_files_to_watched(DeleteFilesParams {
        files: vec![FileDelete {
            uri: "file:///gone.ws".to_string(),
        }],
    })
    .changes;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].typ, FileChangeType::DELETED);
    assert_eq!(changes[0].uri.as_str(), "file:///gone.ws");
}

#[test]
fn rename_becomes_delete_old_then_create_new() {
    let changes = renamed_files_to_watched(RenameFilesParams {
        files: vec![FileRename {
            old_uri: "file:///old.ws".to_string(),
            new_uri: "file:///new.ws".to_string(),
        }],
    })
    .changes;
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].typ, FileChangeType::DELETED);
    assert_eq!(changes[0].uri.as_str(), "file:///old.ws");
    assert_eq!(changes[1].typ, FileChangeType::CREATED);
    assert_eq!(changes[1].uri.as_str(), "file:///new.ws");
}

#[test]
fn unparseable_uris_are_skipped() {
    let changes = created_files_to_watched(CreateFilesParams {
        files: vec![
            FileCreate {
                uri: "not a uri".to_string(),
            },
            FileCreate {
                uri: "file:///ok.ws".to_string(),
            },
        ],
    })
    .changes;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].uri.as_str(), "file:///ok.ws");
}
