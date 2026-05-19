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
    let segments = crate::indexing::build_index_segments(None, &[], true);
    assert!(segments.is_empty());
}

#[test]
fn build_index_segments_game_dir_only() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_game_only");
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &[], true);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].0, "gameDirectory");
    assert!(!segments[0].2);
}

#[test]
fn build_index_segments_auto_loads_mod_shared_imports_when_present() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_msi_present");
    let msi = game_dir.join("Mods").join("modSharedImports");
    std::fs::create_dir_all(&msi).expect("mkdir mods");
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &[], true);
    let labels: Vec<&str> = segments.iter().map(|(l, _, _)| *l).collect();
    assert!(labels.contains(&"modSharedImports"));
    let msi_seg = segments
        .iter()
        .find(|(l, _, _)| *l == "modSharedImports")
        .unwrap();
    assert!(
        msi_seg.2,
        "modSharedImports segment must be flagged as auto-loaded"
    );
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
}

#[test]
fn build_index_segments_skips_mod_shared_imports_when_flag_off() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_msi_flag_off");
    let msi = game_dir.join("Mods").join("modSharedImports");
    std::fs::create_dir_all(&msi).expect("mkdir mods");
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &[], false);
    let labels: Vec<&str> = segments.iter().map(|(l, _, _)| *l).collect();
    assert!(!labels.contains(&"modSharedImports"));
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
}

#[test]
fn build_index_segments_skips_missing_extra_dir() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_extra_missing");
    let missing = std::env::temp_dir().join("ws_test_segments_definitely_not_a_dir_xyz");
    std::fs::remove_dir_all(&missing).ok();
    let extras = vec![missing];
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &extras, false);
    let labels: Vec<&str> = segments.iter().map(|(l, _, _)| *l).collect();
    assert!(!labels.contains(&"additionalScriptDirectory"));
}

#[test]
fn build_index_segments_dedups_extra_that_overlaps_mod_shared_imports() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_dedup");
    let msi = game_dir.join("Mods").join("modSharedImports");
    std::fs::create_dir_all(&msi).expect("mkdir mods");
    let extras = vec![msi.clone()];
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &extras, true);
    let msi_segs: Vec<_> = segments.iter().filter(|(_, p, _)| p == &msi).collect();
    assert_eq!(msi_segs.len(), 1, "overlapping path must appear once");
    assert_eq!(msi_segs[0].0, "modSharedImports");
    assert!(msi_segs[0].2, "first-inserted (modSharedImports) wins");
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
}

#[cfg(test)]
mod watched_files {
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
            crate::convert::canonical_uri(&opened).expect("canonical uri builds");
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
}

#[cfg(test)]
mod concurrent_doc_ops {
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use arc_swap::ArcSwap;
    use async_lsp::router::Router;
    use async_lsp::{ClientSocket, LanguageServer};
    use lsp_types::{
        DidChangeTextDocumentParams, DidOpenTextDocumentParams, Position, Range,
        TextDocumentContentChangeEvent, TextDocumentItem, Url, VersionedTextDocumentIdentifier,
    };
    use tokio::sync::{mpsc, Mutex};
    use witcherscript_language::builtins::load_builtins_index;
    use witcherscript_language::resolve::WorkspaceIndex;
    use witcherscript_language::script_env::ScriptEnvironment;

    use crate::backend::{Backend, DocOp};
    use crate::config::Config;

    fn make_backend() -> (Backend, mpsc::UnboundedReceiver<DocOp>) {
        let (_main_loop, client) =
            async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
        let (doc_ops_tx, doc_ops_rx) = mpsc::unbounded_channel();
        let backend = Backend {
            client,
            config: Arc::new(ArcSwap::from_pointee(Config {
                diagnostics_enabled: false,
                ..Config::default()
            })),
            documents: Arc::new(Mutex::new(HashMap::new())),
            published_diagnostics: Arc::new(Mutex::new(HashMap::new())),
            workspace_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            workspace_documents: Arc::new(Mutex::new(HashMap::new())),
            workspace_roots: Arc::new(Mutex::new(Vec::new())),
            files_exclude: Arc::new(Mutex::new(Vec::new())),
            base_scripts_path: Arc::new(Mutex::new(None)),
            additional_script_dirs: Arc::new(Mutex::new(Vec::new())),
            base_scripts_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            base_scripts_documents: Arc::new(Mutex::new(HashMap::new())),
            builtins_index: Arc::new(load_builtins_index()),
            script_env: Arc::new(Mutex::new(ScriptEnvironment::default())),
            cst_diag_cache: Arc::new(Mutex::new(HashMap::new())),
            initial_index_done: Arc::new(AtomicBool::new(false)),
            doc_ops_tx,
        };
        (backend, doc_ops_rx)
    }

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

    fn change_params(
        uri: &Url,
        version: i32,
        start: (u32, u32),
        end: (u32, u32),
        text: &str,
    ) -> DidChangeTextDocumentParams {
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: start.0,
                        character: start.1,
                    },
                    end: Position {
                        line: end.0,
                        character: end.1,
                    },
                }),
                range_length: None,
                text: text.to_string(),
            }],
        }
    }

    async fn wait_for(backend: &Backend, uri: &Url, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            {
                let docs = backend.documents.lock().await;
                if docs.get(uri).map(|d| d.source.as_str()) == Some(expected) {
                    return;
                }
            }
            if Instant::now() > deadline {
                let docs = backend.documents.lock().await;
                panic!(
                    "consumer did not produce expected source within 5s; got {:?}",
                    docs.get(uri).map(|d| d.source.clone()),
                );
            }
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn rapid_did_change_submissions_apply_in_order() {
        let (mut backend, mut doc_ops_rx) = make_backend();
        let consumer_backend = backend.clone();
        tokio::spawn(async move {
            while let Some(op) = doc_ops_rx.recv().await {
                consumer_backend.dispatch_doc_op(op).await;
            }
        });

        let uri: Url = "file:///rapid_changes.ws".parse().unwrap();
        let _ = backend.did_open(open_params(&uri, "abc"));

        let _ = backend.did_change(change_params(&uri, 2, (0, 3), (0, 3), "def"));
        let _ = backend.did_change(change_params(&uri, 3, (0, 5), (0, 6), ""));

        wait_for(&backend, &uri, "abcde").await;
    }

    #[tokio::test]
    async fn interleaved_changes_across_two_documents_apply_in_order() {
        let (mut backend, mut doc_ops_rx) = make_backend();
        let consumer_backend = backend.clone();
        tokio::spawn(async move {
            while let Some(op) = doc_ops_rx.recv().await {
                consumer_backend.dispatch_doc_op(op).await;
            }
        });

        let uri_a: Url = "file:///a.ws".parse().unwrap();
        let uri_b: Url = "file:///b.ws".parse().unwrap();
        let _ = backend.did_open(open_params(&uri_a, "a"));
        let _ = backend.did_open(open_params(&uri_b, "b"));

        let _ = backend.did_change(change_params(&uri_a, 2, (0, 1), (0, 1), "X"));
        let _ = backend.did_change(change_params(&uri_b, 2, (0, 1), (0, 1), "Y"));
        let _ = backend.did_change(change_params(&uri_a, 3, (0, 2), (0, 2), "X"));
        let _ = backend.did_change(change_params(&uri_b, 3, (0, 2), (0, 2), "Y"));

        wait_for(&backend, &uri_a, "aXX").await;
        wait_for(&backend, &uri_b, "bYY").await;
    }
}
