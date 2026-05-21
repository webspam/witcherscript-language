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
}

#[cfg(test)]
mod concurrent_doc_ops {
    use std::collections::{HashMap, HashSet};
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
            legacy_script_dirs: Arc::new(Mutex::new(Vec::new())),
            legacy_indexed_uris: Arc::new(Mutex::new(HashSet::new())),
            legacy_replacements: Arc::new(Mutex::new(HashMap::new())),
            sent_legacy_status: Arc::new(Mutex::new(HashMap::new())),
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

#[cfg(test)]
mod legacy_routing {
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use arc_swap::ArcSwap;
    use async_lsp::router::Router;
    use async_lsp::ClientSocket;
    use lsp_types::{
        DidCloseTextDocumentParams, FileChangeType, FileEvent, TextDocumentIdentifier, Url,
    };
    use tokio::sync::{mpsc, Mutex};
    use witcherscript_language::builtins::load_builtins_index;
    use witcherscript_language::diagnostics::{
        collect_base_script_conflict_diagnostics, collect_duplicate_symbol_diagnostics,
    };
    use witcherscript_language::files::canonical_uri;
    use witcherscript_language::resolve::WorkspaceIndex;
    use witcherscript_language::script_env::ScriptEnvironment;

    use crate::backend::{Backend, DocOp};
    use crate::config::Config;
    use crate::indexing::{legacy_base_replacements, legacy_replaces_base};
    use crate::watcher::event_touches_legacy_dir;

    pub(super) struct LocalTempDir {
        path: PathBuf,
    }

    impl LocalTempDir {
        pub(super) fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(name);
            std::fs::remove_dir_all(&path).ok();
            std::fs::create_dir_all(&path).expect("mkdir tempdir");
            Self { path }
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for LocalTempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.path).ok();
        }
    }

    pub(super) fn make_backend() -> Backend {
        let (_main_loop, client) =
            async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
        let (doc_ops_tx, _doc_ops_rx) = mpsc::unbounded_channel();
        Backend {
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
            legacy_script_dirs: Arc::new(Mutex::new(Vec::new())),
            legacy_indexed_uris: Arc::new(Mutex::new(HashSet::new())),
            legacy_replacements: Arc::new(Mutex::new(HashMap::new())),
            sent_legacy_status: Arc::new(Mutex::new(HashMap::new())),
            base_scripts_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            base_scripts_documents: Arc::new(Mutex::new(HashMap::new())),
            builtins_index: Arc::new(load_builtins_index()),
            script_env: Arc::new(Mutex::new(ScriptEnvironment::default())),
            cst_diag_cache: Arc::new(Mutex::new(HashMap::new())),
            initial_index_done: Arc::new(AtomicBool::new(false)),
            doc_ops_tx,
        }
    }

    pub(super) fn write_script(dir: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir parent");
        }
        std::fs::write(&path, contents).expect("write script");
        path
    }

    fn make_game_dir(temp: &Path, rel: &str, contents: &str) -> (PathBuf, Url) {
        let game_dir = temp.join("game");
        let full_rel = Path::new("content")
            .join("content0")
            .join("scripts")
            .join(rel);
        let path = write_script(&game_dir, full_rel.to_str().unwrap(), contents);
        let url = Url::from_file_path(&path).expect("base path -> url");
        (game_dir, url)
    }

    async fn indexed_legacy_override(name: &str) -> (LocalTempDir, Backend, Url, Url) {
        let temp = LocalTempDir::new(name);
        let (game_dir, _base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let override_path = write_script(
            &legacy_dir,
            "game/r4Player.ws",
            "class CR4Player {}\n// legacy\n",
        );
        let new_path = write_script(&legacy_dir, "game/MyNewMod.ws", "class CMyNewMod {}\n");
        let override_url = Url::from_file_path(&override_path).expect("override path -> url");
        let new_url = Url::from_file_path(&new_path).expect("new path -> url");

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir];
        backend.index_base_scripts().await;
        (temp, backend, override_url, new_url)
    }

    #[test]
    fn legacy_replaces_base_matches_same_relpath() {
        assert!(legacy_replaces_base(
            "file:///game/content/content0/scripts/game/r4Player.ws",
            "file:///mod/legacy/game/r4Player.ws",
        ));
    }

    #[test]
    fn legacy_replaces_base_requires_path_separator() {
        assert!(!legacy_replaces_base(
            "file:///game/content/content0/scripts/game/r4Player.ws",
            "file:///mod/legacy/Xgame/r4Player.ws",
        ));
    }

    #[test]
    fn legacy_replaces_base_skips_base_without_scripts_segment() {
        assert!(!legacy_replaces_base(
            "file:///game/r4Player.ws",
            "file:///mod/legacy/r4Player.ws",
        ));
    }

    #[test]
    fn legacy_replaces_base_basename_only_no_match() {
        assert!(!legacy_replaces_base(
            "file:///game/content/content0/scripts/game/r4Player.ws",
            "file:///mod/legacy/r4Player.ws",
        ));
    }

    #[test]
    fn event_touches_legacy_dir_true_inside() {
        let temp = LocalTempDir::new("ws_event_touches_legacy_dir_true_inside");
        let file = temp.path().join("game").join("r4Player.ws");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        let event = FileEvent {
            uri: Url::from_file_path(&file).unwrap(),
            typ: FileChangeType::CHANGED,
        };
        assert!(event_touches_legacy_dir(
            &event,
            &[temp.path().to_path_buf()]
        ));
    }

    #[test]
    fn event_touches_legacy_dir_false_outside() {
        let temp = LocalTempDir::new("ws_event_touches_legacy_dir_false_outside");
        let legacy = temp.path().join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        let elsewhere = temp.path().join("workspace").join("foo.ws");
        std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        std::fs::write(&elsewhere, "").unwrap();
        let event = FileEvent {
            uri: Url::from_file_path(&elsewhere).unwrap(),
            typ: FileChangeType::CHANGED,
        };
        assert!(!event_touches_legacy_dir(&event, &[legacy]));
    }

    #[test]
    fn event_touches_legacy_dir_empty_dirs_returns_false() {
        let temp = LocalTempDir::new("ws_event_touches_legacy_dir_empty_dirs_returns_false");
        let file = temp.path().join("foo.ws");
        std::fs::write(&file, "").unwrap();
        let event = FileEvent {
            uri: Url::from_file_path(&file).unwrap(),
            typ: FileChangeType::CHANGED,
        };
        assert!(!event_touches_legacy_dir(&event, &[]));
    }

    #[tokio::test]
    async fn matching_legacy_file_drops_base_and_lands_in_workspace() {
        let temp = LocalTempDir::new("ws_matching_legacy_file_drops_base");
        let (game_dir, base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let legacy_path = write_script(
            &legacy_dir,
            "game/r4Player.ws",
            "class CR4Player {}\n// legacy\n",
        );
        let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir.clone()];

        backend.index_base_scripts().await;

        let base_docs = backend.base_scripts_documents.lock().await;
        assert!(
            !base_docs.contains_key(base_url.as_str()),
            "base script should be replaced; got keys {:?}",
            base_docs.keys().collect::<Vec<_>>()
        );

        let ws_docs = backend.workspace_documents.lock().await;
        assert!(
            ws_docs.contains_key(legacy_url.as_str()),
            "legacy file should be in workspace_documents; got keys {:?}",
            ws_docs.keys().collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn deleting_a_legacy_file_removes_it_from_the_workspace() {
        let temp = LocalTempDir::new("ws_deleting_legacy_file_removes_it");
        let (game_dir, base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let legacy_path = write_script(
            &legacy_dir,
            "game/r4Player.ws",
            "class CR4Player {}\n// legacy\n",
        );
        let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

        backend.index_base_scripts().await;
        assert!(
            backend
                .workspace_documents
                .lock()
                .await
                .contains_key(legacy_url.as_str()),
            "legacy file should be indexed into the workspace first"
        );

        std::fs::remove_file(&legacy_path).expect("remove legacy file");
        backend.index_base_scripts().await;

        assert!(
            !backend
                .workspace_documents
                .lock()
                .await
                .contains_key(legacy_url.as_str()),
            "a deleted legacy file must not linger in workspace_documents"
        );
        assert!(
            backend.legacy_indexed_uris.lock().await.is_empty(),
            "tracked legacy URIs must be cleared once the file is gone"
        );
        assert!(
            backend
                .base_scripts_documents
                .lock()
                .await
                .contains_key(base_url.as_str()),
            "the base script returns to the base index once nothing overrides it"
        );
    }

    #[tokio::test]
    async fn unmatched_legacy_file_still_lands_in_workspace() {
        let temp = LocalTempDir::new("ws_unmatched_legacy_file_lands_in_workspace");
        let (game_dir, base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let legacy_path = write_script(
            &legacy_dir,
            "game/MyMod.ws",
            "@addMethod(CR4Player)\nfunction Hi() {}\n",
        );
        let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir.clone()];

        backend.index_base_scripts().await;

        let base_docs = backend.base_scripts_documents.lock().await;
        assert!(
            base_docs.contains_key(base_url.as_str()),
            "unmatched legacy file must not remove the base script"
        );

        let ws_docs = backend.workspace_documents.lock().await;
        assert!(
            ws_docs.contains_key(legacy_url.as_str()),
            "annotated legacy file should be in workspace_documents"
        );
    }

    #[tokio::test]
    async fn base_script_conflict_silent_on_matched_legacy_file() {
        let temp = LocalTempDir::new("ws_base_script_conflict_silent_on_matched_legacy");
        let (game_dir, _base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let _ = write_script(
            &legacy_dir,
            "game/r4Player.ws",
            "class CR4Player {}\n// legacy\n",
        );

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

        backend.index_base_scripts().await;

        let ws = backend.workspace_index.lock().await;
        let base = backend.base_scripts_index.lock().await;
        let legacy_dirs = backend.legacy_script_dirs.lock().await.clone();
        let diagnostics = collect_base_script_conflict_diagnostics(&ws, &base, &legacy_dirs);
        assert!(
            diagnostics.is_empty(),
            "matched legacy file must not trigger base_script_conflict; got {diagnostics:?}",
        );
    }

    #[tokio::test]
    async fn opening_an_overridden_base_script_keeps_it_out_of_the_workspace() {
        let temp = LocalTempDir::new("ws_open_overridden_base_no_dup");
        let (game_dir, base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let _ = write_script(
            &legacy_dir,
            "game/r4Player.ws",
            "class CR4Player {}\n// legacy\n",
        );

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir];
        backend.index_base_scripts().await;

        backend
            .update_open_document(base_url.clone(), "class CR4Player {}\n".to_string())
            .await;

        let ws = backend.workspace_index.lock().await;
        let base = backend.base_scripts_index.lock().await;
        let legacy_dirs = backend.legacy_script_dirs.lock().await.clone();

        assert!(
            collect_duplicate_symbol_diagnostics(&ws).is_empty(),
            "opening the overridden base script must not create a workspace duplicate",
        );
        assert!(
            collect_base_script_conflict_diagnostics(&ws, &base, &legacy_dirs).is_empty(),
            "the legacy override must not be flagged once both files are loaded",
        );
        assert!(
            base.documents().any(|(uri, _)| uri == base_url.as_str()),
            "the opened base script should be indexed as a base script",
        );
    }

    #[tokio::test]
    async fn reindexing_keeps_an_open_legacy_file_indexed() {
        let temp = LocalTempDir::new("ws_reindex_keeps_open_legacy");
        let (game_dir, _base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let legacy_path = write_script(&legacy_dir, "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

        backend.index_base_scripts().await;
        backend
            .update_open_document(legacy_url.clone(), "class CR4Player {}\n".to_string())
            .await;
        backend.index_base_scripts().await;

        assert!(
            backend
                .workspace_index
                .lock()
                .await
                .documents()
                .any(|(uri, _)| uri == legacy_url.as_str()),
            "an open legacy file must survive a re-index",
        );
    }

    #[tokio::test]
    async fn reindexing_keeps_an_open_overridden_base_script_indexed() {
        let temp = LocalTempDir::new("ws_reindex_keeps_open_overridden_base");
        let (game_dir, base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let _ = write_script(
            &legacy_dir,
            "game/r4Player.ws",
            "class CR4Player {}\n// legacy\n",
        );

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

        backend.index_base_scripts().await;
        backend
            .update_open_document(base_url.clone(), "class CR4Player {}\n".to_string())
            .await;
        backend.index_base_scripts().await;

        assert!(
            backend
                .base_scripts_index
                .lock()
                .await
                .documents()
                .any(|(uri, _)| uri == base_url.as_str()),
            "an open, legacy-overridden base script must survive a re-index",
        );
    }

    #[test]
    fn legacy_base_replacements_maps_only_real_overrides() {
        struct Case {
            name: &'static str,
            base: &'static [&'static str],
            legacy: &'static [&'static str],
            expect_skip: &'static [&'static str],
            expect_map: &'static [(&'static str, &'static str)],
        }
        let cases = [
            Case {
                name: "legacy file at the same game-relative path replaces the base script",
                base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
                legacy: &["file:///mod/legacy/game/r4Player.ws"],
                expect_skip: &["file:///game/content/content0/scripts/game/r4Player.ws"],
                expect_map: &[("file:///mod/legacy/game/r4Player.ws", "game/r4Player.ws")],
            },
            Case {
                name: "brand-new script in a legacy folder replaces nothing",
                base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
                legacy: &["file:///mod/legacy/game/MyNewMod.ws"],
                expect_skip: &[],
                expect_map: &[],
            },
            Case {
                name: "same basename but a different relative path replaces nothing",
                base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
                legacy: &["file:///mod/legacy/other/r4Player.ws"],
                expect_skip: &[],
                expect_map: &[],
            },
        ];
        for c in cases {
            let base: Vec<String> = c.base.iter().map(|s| s.to_string()).collect();
            let legacy: Vec<String> = c.legacy.iter().map(|s| s.to_string()).collect();
            let (skip, map) = legacy_base_replacements(&base, &legacy);
            let expect_skip: HashSet<String> =
                c.expect_skip.iter().map(|s| s.to_string()).collect();
            let expect_map: HashMap<String, String> = c
                .expect_map
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            assert_eq!(skip, expect_skip, "case '{}': skip set mismatch", c.name);
            assert_eq!(
                map, expect_map,
                "case '{}': replacement map mismatch",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn index_base_scripts_records_only_real_legacy_overrides() {
        let (_temp, backend, override_url, new_url) =
            indexed_legacy_override("ws_legacy_replacements_map").await;

        let map = backend.legacy_replacements.lock().await;
        let override_key = canonical_uri(&override_url).expect("canonical override uri");
        assert_eq!(
            map.get(&override_key).map(String::as_str),
            Some("game/r4Player.ws"),
            "a legacy file overriding a base script must record the replaced path",
        );
        let new_key = canonical_uri(&new_url).expect("canonical new uri");
        assert!(
            !map.contains_key(&new_key),
            "a brand-new script in a legacy folder must not be recorded as a replacement",
        );
    }

    #[tokio::test]
    async fn opening_a_legacy_override_marks_it_as_replacing_a_base_script() {
        let (_temp, backend, override_url, new_url) =
            indexed_legacy_override("ws_legacy_status_open").await;

        backend
            .update_open_document(override_url.clone(), "class CR4Player {}\n".to_string())
            .await;
        backend
            .update_open_document(new_url.clone(), "class CMyNewMod {}\n".to_string())
            .await;

        let sent = backend.sent_legacy_status.lock().await;
        let override_status = sent
            .get(&override_url)
            .expect("status sent for the override file");
        assert!(
            override_status.replaces_base_script,
            "an open legacy override must be reported as replacing a base script",
        );
        assert_eq!(
            override_status.replaced_script_path.as_deref(),
            Some("game/r4Player.ws"),
        );
        let new_status = sent.get(&new_url).expect("status sent for the new file");
        assert!(
            !new_status.replaces_base_script,
            "a brand-new script in a legacy folder must not be reported as replacing a base script",
        );
    }

    #[tokio::test]
    async fn closing_a_legacy_override_keeps_its_status_dedup_entry() {
        let (_temp, backend, override_url, _new_url) =
            indexed_legacy_override("ws_legacy_status_close").await;
        backend
            .update_open_document(override_url.clone(), "class CR4Player {}\n".to_string())
            .await;

        backend
            .dispatch_doc_op(DocOp::Close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: override_url.clone(),
                },
            }))
            .await;

        assert!(
            backend
                .sent_legacy_status
                .lock()
                .await
                .contains_key(&override_url),
            "closing a file must keep its status dedup entry, or an unrelated edit \
             would re-push a notification for the closed file",
        );
    }

    #[tokio::test]
    async fn additional_script_dir_overlapping_legacy_logs_and_wins_as_legacy() {
        let temp = LocalTempDir::new("ws_additional_overlapping_legacy_wins_as_legacy");
        let (game_dir, base_url) =
            make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
        let legacy_dir = temp.path().join("legacy");
        let legacy_path = write_script(
            &legacy_dir,
            "game/r4Player.ws",
            "class CR4Player {}\n// override\n",
        );
        let legacy_url = Url::from_file_path(&legacy_path).expect("legacy path -> url");

        let backend = make_backend();
        *backend.base_scripts_path.lock().await = Some(game_dir);
        *backend.additional_script_dirs.lock().await = vec![legacy_dir.clone()];
        *backend.legacy_script_dirs.lock().await = vec![legacy_dir];

        backend.index_base_scripts().await;

        let base_docs = backend.base_scripts_documents.lock().await;
        assert!(
            !base_docs.contains_key(base_url.as_str()),
            "legacy semantics must win when the same dir appears in both lists"
        );
        assert!(
            !base_docs.contains_key(legacy_url.as_str()),
            "legacy file must not be loaded as a base overlay"
        );

        let ws_docs = backend.workspace_documents.lock().await;
        assert!(
            ws_docs.contains_key(legacy_url.as_str()),
            "legacy file must land in workspace_documents"
        );
    }
}

#[cfg(test)]
mod workspace_folder_changes {
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
}

#[cfg(test)]
mod file_operation_events {
    use std::path::Path;

    use lsp_types::{DeleteFilesParams, FileDelete, FileRename, RenameFilesParams, Url};

    use super::legacy_routing::{make_backend, write_script, LocalTempDir};
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
}
