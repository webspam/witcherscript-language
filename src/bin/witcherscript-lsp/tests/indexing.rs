use witcherscript_language::document::parse_document;
use witcherscript_language::resolve::WorkspaceIndex;

#[test]
#[cfg(windows)]
fn opening_a_workspace_indexed_file_does_not_self_conflict() {
    use crate::indexing::index_open_document;
    use lsp_types::Url;
    use witcherscript_language::diagnostics::collect_duplicate_symbol_diagnostics;

    let document = parse_document("function Foo() {}\n").expect("document should parse");
    let mut index = WorkspaceIndex::default();

    // The editor opens the file under its own (percent-encoded) spelling, while
    // index_workspace keys the same file via Url::from_file_path.
    let opened = Url::parse("file:///c%3A/proj/foo.ws").expect("uri should parse");
    let indexed_uri = Url::from_file_path(opened.to_file_path().unwrap())
        .expect("path should convert back to a URI");
    assert_ne!(
        indexed_uri.as_str(),
        opened.as_str(),
        "test must exercise a real client-vs-canonical spelling mismatch"
    );

    index.update_document(indexed_uri.as_str(), &document);
    index_open_document(&mut index, &opened, &document);

    assert!(
        collect_duplicate_symbol_diagnostics(&index).is_empty(),
        "a workspace-indexed file that is then opened must not be flagged as a duplicate of itself"
    );
}

#[test]
fn build_index_segments_empty_inputs() {
    let segments = crate::indexing::build_index_segments(None, &[]);
    assert!(segments.is_empty());
}

#[test]
fn build_index_segments_game_dir_only() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_game_only");
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &[]);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].0, "gameDirectory");
}

#[test]
fn build_index_segments_skips_missing_extra_dir() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_extra_missing");
    let missing = std::env::temp_dir().join("ws_test_segments_definitely_not_a_dir_xyz");
    std::fs::remove_dir_all(&missing).ok();
    let extras = vec![missing];
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &extras);
    let labels: Vec<&str> = segments.iter().map(|(l, _)| *l).collect();
    assert!(!labels.contains(&"additionalScriptDirectory"));
}

#[test]
fn build_index_segments_never_emits_mod_shared_imports() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_no_msi");
    let msi = game_dir.join("Mods").join("modSharedImports");
    std::fs::create_dir_all(&msi).expect("mkdir mods");
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &[]);
    let labels: Vec<&str> = segments.iter().map(|(l, _)| *l).collect();
    assert!(
        !labels.contains(&"modSharedImports"),
        "modSharedImports must be routed through legacy dirs, not base segments"
    );
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
}

#[test]
fn mod_shared_imports_dir_detects_present_dir() {
    let game_dir = std::env::temp_dir().join("ws_test_msi_detect");
    let msi = game_dir.join("Mods").join("modSharedImports");
    std::fs::create_dir_all(&msi).expect("mkdir mods");
    assert_eq!(
        crate::indexing::mod_shared_imports_dir(&game_dir).as_deref(),
        Some(msi.as_path())
    );
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
}

#[test]
fn mod_shared_imports_dir_none_when_absent() {
    let game_dir = std::env::temp_dir().join("ws_test_msi_absent");
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
    assert!(crate::indexing::mod_shared_imports_dir(&game_dir).is_none());
}

#[cfg(test)]
mod watched_files;

#[cfg(test)]
mod concurrent_doc_ops;

#[cfg(test)]
mod legacy_helpers;

#[cfg(test)]
mod legacy_predicates;

#[cfg(test)]
mod legacy_reindex;

#[cfg(test)]
mod legacy_routing;

#[cfg(test)]
mod workspace_folder_changes;

#[cfg(test)]
mod diagnostics_scope;

#[cfg(test)]
mod file_operation_events;

#[cfg(test)]
mod loose_files;
