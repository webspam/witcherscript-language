use std::collections::HashSet;
use std::path::PathBuf;

use lsp_types::{FileChangeType, FileEvent, Url};
use witcherscript_language::files::ExcludeFilter;

use crate::watcher::{classify_watched_event, WatchedEvent};

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
fn delete_of_open_file_returns_remove() {
    let url = uri_under_root("open.ws");
    let mut open = HashSet::new();
    open.insert(url.to_string());
    let decision = classify_watched_event(
        &event(url.as_str(), FileChangeType::DELETED),
        &open,
        &no_filter(),
    );
    assert_eq!(
        decision,
        Some(WatchedEvent::Remove {
            canonical: url.to_string()
        })
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
#[cfg(windows)]
fn canonicalises_percent_encoded_uri_for_open_file_skip() {
    let opened = Url::parse("file:///c%3A/proj/foo.ws").expect("client uri parses");
    let canonical_opened =
        witcherscript_language::files::canonical_uri(&opened).expect("canonical uri builds");
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
