use std::collections::HashSet;
use std::path::{Path, PathBuf};

use lsp_types::{FileChangeType, FileEvent, Url};
use witcherscript_language::files::{ExcludeFilter, canonical_uri};

use super::legacy_helpers::write_script;
use crate::tests::support::{LocalTempDir, make_backend};
use crate::watcher::{WatchedEvent, classify_watched_event};

fn event(uri: &str, typ: FileChangeType) -> FileEvent {
    FileEvent {
        uri: Url::parse(uri).expect("uri parses"),
        typ,
    }
}

fn workspace_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("C:\\workspace")
    } else {
        PathBuf::from("/workspace")
    }
}

fn uri_under_root(rel: &str) -> Url {
    Url::from_file_path(workspace_root().join(rel)).expect("uri builds")
}

fn no_filter() -> ExcludeFilter {
    ExcludeFilter::new(&[workspace_root()], &[])
}

#[test]
fn created_event_returns_upsert() {
    let url = uri_under_root("foo.ws");
    let canonical = url.to_string();
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::CREATED),
        &HashSet::new(),
        &no_filter(),
    );
    let Some(WatchedEvent::Upsert {
        canonical: got,
        path,
    }) = decision
    else {
        panic!("expected Upsert, got {decision:?}");
    };
    assert_eq!(got, canonical);
    assert!(path.ends_with("foo.ws"));
}

#[test]
fn changed_event_returns_upsert() {
    let url = uri_under_root("bar.ws");
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::CHANGED),
        &HashSet::new(),
        &no_filter(),
    );
    assert!(matches!(decision, Some(WatchedEvent::Upsert { .. })));
}

#[test]
fn deleted_event_returns_remove() {
    let url = uri_under_root("gone.ws");
    let canonical = url.to_string();
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::DELETED),
        &HashSet::new(),
        &no_filter(),
    );
    assert_eq!(
        decision,
        Some(WatchedEvent::Remove {
            canonical: canonical.clone()
        })
    );
}

#[test]
fn deleted_event_ignores_exclude_filter() {
    let url = uri_under_root("excluded/gone.ws");
    let filter = ExcludeFilter::new(&[workspace_root()], &["excluded/**".to_string()]);
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::DELETED),
        &HashSet::new(),
        &filter,
    );
    assert!(matches!(decision, Some(WatchedEvent::Remove { .. })));
}

#[test]
fn skips_event_for_open_file() {
    let url = uri_under_root("open.ws");
    let mut open = HashSet::new();
    open.insert(url.to_string());
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::CHANGED),
        &open,
        &no_filter(),
    );
    assert_eq!(decision, None);
}

#[test]
fn delete_of_open_file_is_skipped() {
    let url = uri_under_root("open.ws");
    let mut open = HashSet::new();
    open.insert(url.to_string());
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::DELETED),
        &open,
        &no_filter(),
    );
    assert_eq!(
        decision, None,
        "deleting an open file on disk (e.g. branch switch) must not drop the editor buffer from the index"
    );
}

#[test]
fn skips_event_for_excluded_path() {
    let url = uri_under_root("vendor/lib.ws");
    let filter = ExcludeFilter::new(&[workspace_root()], &["vendor/**".to_string()]);
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::CREATED),
        &HashSet::new(),
        &filter,
    );
    assert_eq!(decision, None);
}

#[test]
fn skips_event_for_non_ws_extension() {
    let url = uri_under_root("notes.txt");
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::CREATED),
        &HashSet::new(),
        &no_filter(),
    );
    assert_eq!(decision, None);
}

#[test]
fn deleted_directory_event_returns_remove_tree() {
    let url = Url::from_file_path(workspace_root().join("pack")).expect("dir uri builds");
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::DELETED),
        &HashSet::new(),
        &no_filter(),
    );
    assert!(
        matches!(decision, Some(WatchedEvent::RemoveTree { .. })),
        "a deleted directory must classify as a tree removal, got {decision:?}"
    );
}

#[test]
fn deleted_excluded_tree_is_skipped() {
    let url = Url::from_file_path(workspace_root().join("vendor/cache")).expect("dir uri builds");
    let filter = ExcludeFilter::new(&[workspace_root()], &["vendor/**".to_string()]);
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::DELETED),
        &HashSet::new(),
        &filter,
    );
    assert_eq!(
        decision, None,
        "a deleted path under an excluded tree must not trigger a removal scan"
    );
}

#[test]
fn directory_delete_drops_indexed_files_under_it() {
    let temp = LocalTempDir::new("ws_dir_delete_prefix");
    let backend = make_backend();
    backend.set_workspace_roots(vec![temp.path().to_path_buf()]);

    let nested = write_script(temp.path(), "pack/sub/A.ws", "class CA {}\n");
    let direct = write_script(temp.path(), "pack/B.ws", "class CB {}\n");
    let outside = write_script(temp.path(), "other/C.ws", "class CC {}\n");
    let file_event = |path: &Path, typ| FileEvent {
        uri: Url::from_file_path(path).expect("path -> url"),
        typ,
    };
    backend.apply_watched_file_events(vec![
        file_event(&nested, FileChangeType::CREATED),
        file_event(&direct, FileChangeType::CREATED),
        file_event(&outside, FileChangeType::CREATED),
    ]);
    let canon = |path: &Path| canonical_uri(&Url::from_file_path(path).expect("path -> url"));
    assert!(
        backend
            .snapshot()
            .workspace_documents
            .contains_key(&canon(&nested)),
        "sanity: upserted file must be indexed before the directory delete"
    );

    std::fs::remove_dir_all(temp.path().join("pack")).expect("delete pack dir");
    backend.apply_watched_file_events(vec![file_event(
        &temp.path().join("pack"),
        FileChangeType::DELETED,
    )]);

    let snap = backend.snapshot();
    assert!(
        !snap.workspace_documents.contains_key(&canon(&nested)),
        "nested file under deleted directory must leave the index"
    );
    assert!(
        !snap.workspace_documents.contains_key(&canon(&direct)),
        "direct child of deleted directory must leave the index"
    );
    assert!(
        snap.workspace_documents.contains_key(&canon(&outside)),
        "file outside the deleted directory must remain indexed"
    );
    assert!(
        !backend
            .workspace_known_files
            .lock()
            .contains(&canon(&nested)),
        "known-files bookkeeping must drop files under the deleted directory"
    );
}

#[test]
#[cfg(windows)]
fn canonicalises_percent_encoded_uri_for_open_file_skip() {
    let opened = Url::parse("file:///c%3A/proj/foo.ws").expect("client uri parses");
    let canonical_opened = witcherscript_language::files::canonical_uri(&opened);
    assert_ne!(canonical_opened, opened.as_str());

    let watcher_url =
        Url::from_file_path(opened.to_file_path().unwrap()).expect("path converts back to uri");
    let open_canonical: HashSet<String> = [canonical_opened.clone()].into_iter().collect();
    let filter = ExcludeFilter::new(&[PathBuf::from("C:\\proj")], &[]);

    let decision = classify_watched_event(
        &event(watcher_url.as_str(), FileChangeType::CHANGED),
        &open_canonical,
        &filter,
    );
    assert_eq!(
        decision, None,
        "watcher event for an open file (under different URI spelling) must be skipped"
    );
}
