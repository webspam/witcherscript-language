use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use arc_swap::ArcSwap;
use async_lsp::router::Router;
use async_lsp::{ClientSocket, ErrorCode};
use lsp_types::{
    CompletionContext, CompletionParams, CompletionResponse, CompletionTriggerKind,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DocumentDiagnosticParams,
    DocumentDiagnosticReport, DocumentDiagnosticReportResult, DocumentFormattingParams,
    FormattingOptions, PartialResultParams, Position, Range, SemanticTokensParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Url, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceDiagnosticParams, WorkspaceDiagnosticReportResult,
};
use witcherscript_language::semantic_tokens::collect_semantic_tokens;

use crate::backend::Backend;
use crate::config::{Config, DiagnosticsScope};

fn make_backend() -> Backend {
    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));
    Backend::new(client, config)
}

fn workspace_scope_backend_indexing_pending() -> Backend {
    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: DiagnosticsScope::Workspace,
        ..Config::default()
    }));
    Backend::new(client, config)
}

fn make_workspace_backend() -> Backend {
    let backend = workspace_scope_backend_indexing_pending();
    backend.initial_index_done.store(true, Ordering::Release);
    backend
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

#[test]
fn did_change_applies_incremental_edits() {
    let backend = make_backend();

    let uri: Url = "file:///rapid_changes.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "abc"));
    backend._did_change(change_params(&uri, 2, (0, 3), (0, 3), "def"));
    backend._did_change(change_params(&uri, 3, (0, 5), (0, 6), ""));

    let snap = backend.snapshot();
    let docs = &snap.documents;
    assert_eq!(
        docs.get(&uri).map(|d| d.source.as_str()),
        Some("abcde"),
        "each did_change must compose with prior buffer state and updated line index",
    );
}

#[test]
fn did_change_tracks_each_document_independently() {
    let backend = make_backend();

    let uri_a: Url = "file:///a.ws".parse().unwrap();
    let uri_b: Url = "file:///b.ws".parse().unwrap();
    backend._did_open(open_params(&uri_a, "a"));
    backend._did_open(open_params(&uri_b, "b"));

    backend._did_change(change_params(&uri_a, 2, (0, 1), (0, 1), "X"));
    backend._did_change(change_params(&uri_b, 2, (0, 1), (0, 1), "Y"));
    backend._did_change(change_params(&uri_a, 3, (0, 2), (0, 2), "X"));
    backend._did_change(change_params(&uri_b, 3, (0, 2), (0, 2), "Y"));

    let snap = backend.snapshot();
    let docs = &snap.documents;
    assert_eq!(
        docs.get(&uri_a).map(|d| d.source.as_str()),
        Some("aXX"),
        "edits to one document must not leak into another's buffer",
    );
    assert_eq!(
        docs.get(&uri_b).map(|d| d.source.as_str()),
        Some("bYY"),
        "edits to one document must not leak into another's buffer",
    );
}

// Regression test for #94: when did_change runs synchronously in the notification
// handler, the next handler that locks `documents` must observe the post-change
// source. The old mpsc dispatcher deferred did_change to a worker task, letting a
// concurrent semanticTokens handler read stale `documents` before the worker ran.
#[test]
fn semantic_tokens_after_did_change_sees_new_source() {
    let backend = make_backend();
    let uri: Url = "file:///regression94.ws".parse().unwrap();

    backend._did_open(open_params(&uri, "class C {}\n"));
    backend._did_change(change_params(
        &uri,
        2,
        (0, 0),
        (1, 0),
        "class CRenamed {}\n",
    ));

    let snap = backend.snapshot();
    let document = snap
        .documents
        .get(&uri)
        .expect("document present after change")
        .clone();
    assert_eq!(
        document.source, "class CRenamed {}\n",
        "did_change must have applied before any read can observe `documents`",
    );

    let handles = backend.db_handles_for_with_snapshot(&uri, &snap);
    let db = handles.db();
    let tokens = collect_semantic_tokens(uri.as_str(), document.as_ref(), &db);
    assert!(
        !tokens.is_empty(),
        "semantic tokens must be produced from the post-change source",
    );
}

#[test]
fn compute_workspace_diagnostic_report_bails_when_version_advanced() {
    let backend = make_workspace_backend();
    let uri: Url = "file:///stale.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "class CBroken {\n"));

    let stale_version = backend.state_version.load(Ordering::Acquire);
    backend
        .state_version
        .store(stale_version + 100, Ordering::Release);

    let result = backend.compute_workspace_diagnostic_report(&HashMap::new(), stale_version);
    assert!(
        result.is_none(),
        "stale-version workspace pull must bail instead of returning a report",
    );
}

#[test]
fn compute_diagnostics_for_uri_bails_when_version_advanced() {
    let backend = make_workspace_backend();
    let uri: Url = "file:///stale_pull.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "class CPull {}\n"));

    let snap = backend.snapshot();
    let document = snap
        .documents
        .get(&uri)
        .expect("document present after open")
        .clone();

    let stale_version = backend.state_version.load(Ordering::Acquire);
    backend
        .state_version
        .store(stale_version + 100, Ordering::Release);

    let result = backend.compute_diagnostics_for_uri(&uri, document.as_ref(), stale_version);
    assert!(
        result.is_none(),
        "pull compute must bail when the caller's version is already stale"
    );
}

fn semantic_tokens_params(uri: &Url) -> SemanticTokensParams {
    SemanticTokensParams {
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    }
}

fn document_diagnostic_params(uri: &Url) -> DocumentDiagnosticParams {
    DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn formatting_params(uri: &Url) -> DocumentFormattingParams {
    DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        options: FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..FormattingOptions::default()
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    }
}

#[test]
fn did_change_chains_against_in_flight_edit_after_worker_takes_clone() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);

    let uri: Url = "file:///in_flight_chain.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "abc"));
    backend._did_change(change_params(&uri, 2, (0, 3), (0, 3), "DE"));

    let in_flight = backend
        .clone_pending_for(&uri)
        .expect("pending entry present after did_change");
    assert_eq!(in_flight.source, "abcDE");
    assert!(
        backend.pending_edits.lock().contains_key(&uri),
        "cloning for the worker must not drain the entry; otherwise did_change races against a stale snapshot"
    );

    backend._did_change(change_params(&uri, 3, (0, 5), (0, 5), "x"));

    let pending = backend.pending_edits.lock();
    let chained = pending
        .get(&uri)
        .expect("pending entry still present after the second did_change");
    assert_eq!(
        chained.source, "abcDEx",
        "the second did_change must chain onto the in-flight edit, not the stale snapshot",
    );
}

#[test]
fn semantic_tokens_full_bails_when_pending_edit_outranks_snapshot() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);

    let uri: Url = "file:///stale_snap.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "function Foo() {}\n"));
    backend._did_change(change_params(&uri, 2, (0, 9), (0, 12), "Renamed"));

    let snap = backend.snapshot();
    let pre_doc = snap
        .documents
        .get(&uri)
        .expect("document present after open");
    assert_eq!(
        pre_doc.source, "function Foo() {}\n",
        "edit queue must not have published yet"
    );
    assert!(
        backend.pending_target_for(&uri).unwrap() > pre_doc.parse_version,
        "pending target must outrank the snapshot's parse_version"
    );

    let result = backend._semantic_tokens_full(semantic_tokens_params(&uri));
    let Err(err) = result else {
        panic!("expected CONTENT_MODIFIED, got Ok");
    };
    assert_eq!(
        err.code,
        ErrorCode::CONTENT_MODIFIED,
        "stale snapshot must surface CONTENT_MODIFIED"
    );
}

#[test]
fn semantic_tokens_full_unrelated_uri_unaffected_by_pending_edit_elsewhere() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);

    let main: Url = "file:///main.ws".parse().unwrap();
    let utils: Url = "file:///utils.ws".parse().unwrap();
    backend._did_open(open_params(&main, "function Foo() {}\n"));
    backend._did_open(open_params(&utils, "function Bar() {}\n"));

    backend._did_change(change_params(&main, 2, (0, 9), (0, 12), "Renamed"));

    let result = backend._semantic_tokens_full(semantic_tokens_params(&utils));
    assert!(
        matches!(result, Ok(Some(_))),
        "an edit to main.ws must not CONTENT_MODIFIED a read on utils.ws",
    );
}

#[test]
fn document_diagnostic_bails_when_pending_edit_outranks_snapshot() {
    let backend = make_workspace_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);

    let uri: Url = "file:///stale_diag.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "class CDiag {}\n"));
    backend._did_change(change_params(&uri, 2, (0, 6), (0, 11), "CRenamed"));

    let result = backend._document_diagnostic(document_diagnostic_params(&uri));
    let Err(err) = result else {
        panic!("expected CONTENT_MODIFIED, got Ok");
    };
    assert_eq!(
        err.code,
        ErrorCode::CONTENT_MODIFIED,
        "stale snapshot must surface CONTENT_MODIFIED for diagnostics"
    );
}

#[test]
fn document_diagnostic_under_none_scope_returns_empty_for_open_broken_file() {
    let backend = make_backend();
    backend.initial_index_done.store(true, Ordering::Release);
    let uri: Url = "file:///none_scope.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "class CBroken {\n"));

    let report = backend
        ._document_diagnostic(document_diagnostic_params(&uri))
        .expect("None scope must produce a successful response, not an error");
    let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = report
    else {
        panic!("None scope must return a Full report, got {report:?}");
    };
    assert!(
        full.full_document_diagnostic_report.items.is_empty(),
        "None scope must suppress diagnostics for open files, got {:?}",
        full.full_document_diagnostic_report.items,
    );
    assert!(
        full.full_document_diagnostic_report.result_id.is_none(),
        "None scope must not assign a result_id the client could track",
    );
}

#[test]
fn document_diagnostic_serves_unopened_workspace_file_under_workspace_scope() {
    use witcherscript_language::document::parse_document;
    use witcherscript_language::files::canonical_uri;

    let backend = make_workspace_backend();
    let uri: Url = "file:///unopened_ws.ws".parse().unwrap();
    let document = Arc::new(parse_document("class CBroken {\n").expect("fixture parses"));
    backend.publish_compilation(|builder| {
        let (index, docs) = builder.workspace_index_and_docs_mut();
        index.update_document(uri.as_str(), document.as_ref());
        docs.insert(canonical_uri(&uri), document.clone());
    });

    let report = backend
        ._document_diagnostic(document_diagnostic_params(&uri))
        .expect("a workspace-scope pull for an indexed file must succeed");
    let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = report
    else {
        panic!("expected a Full report, got {report:?}");
    };
    assert!(
        !full.full_document_diagnostic_report.items.is_empty(),
        "a restored-but-unopened workspace tab must receive diagnostics from the indexed copy",
    );
}

#[test]
fn document_diagnostic_skips_unopened_workspace_file_under_open_files_scope() {
    use witcherscript_language::document::parse_document;
    use witcherscript_language::files::canonical_uri;

    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: DiagnosticsScope::OpenFiles,
        ..Config::default()
    }));
    let backend = Backend::new(client, config);
    backend.initial_index_done.store(true, Ordering::Release);

    let uri: Url = "file:///unopened_open_scope.ws".parse().unwrap();
    let document = Arc::new(parse_document("class CBroken {\n").expect("fixture parses"));
    backend.publish_compilation(|builder| {
        let (index, docs) = builder.workspace_index_and_docs_mut();
        index.update_document(uri.as_str(), document.as_ref());
        docs.insert(canonical_uri(&uri), document.clone());
    });

    let report = backend
        ._document_diagnostic(document_diagnostic_params(&uri))
        .expect("open-files scope must still produce a successful response");
    let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = report
    else {
        panic!("expected a Full report, got {report:?}");
    };
    assert!(
        full.full_document_diagnostic_report.items.is_empty(),
        "open-files scope must not diagnose a workspace file that is not open",
    );
}

#[test]
fn document_diagnostic_unrelated_uri_unaffected_by_pending_edit_elsewhere() {
    let backend = make_workspace_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);

    let main: Url = "file:///main_diag.ws".parse().unwrap();
    let utils: Url = "file:///utils_diag.ws".parse().unwrap();
    backend._did_open(open_params(&main, "class CMain {}\n"));
    backend._did_open(open_params(&utils, "class CUtils {}\n"));

    backend._did_change(change_params(&main, 2, (0, 6), (0, 11), "CRenamed"));

    let result = backend._document_diagnostic(document_diagnostic_params(&utils));
    assert!(
        result.is_ok(),
        "an edit to main_diag.ws must not CONTENT_MODIFIED a diagnostic on utils_diag.ws",
    );
}

#[test]
fn formatting_reflects_queued_edit_instead_of_bailing() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);

    let uri: Url = "file:///queued_fmt.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "function Foo() {}\n"));
    // Queue a rename plus leading blank lines; neither is published yet.
    backend._did_change(change_params(&uri, 2, (0, 9), (0, 12), "Renamed"));
    backend._did_change(change_params(&uri, 3, (0, 0), (0, 0), "\n\n"));

    let snap = backend.snapshot();
    assert_eq!(
        snap.documents.get(&uri).map(|d| d.source.as_str()),
        Some("function Foo() {}\n"),
        "edits must still be queued, not published, for this test to be meaningful",
    );
    assert!(
        backend.pending_target_for(&uri).unwrap() > snap.documents.get(&uri).unwrap().parse_version,
        "pending edit must outrank the published snapshot",
    );

    let edits = backend
        ._formatting(formatting_params(&uri))
        .expect("formatting must succeed against the queued text, not bail")
        .expect("formatting returns an edit set");
    let new_text = edits
        .first()
        .map(|e| e.new_text.as_str())
        .expect("queued text needed reformatting, so an edit must be produced");
    assert_eq!(
        new_text, "function Renamed() {}\n",
        "formatting must reformat the queued text, including the rename",
    );
}

#[test]
fn publish_compilation_skips_version_bump_for_overlay_only_swap() {
    let backend = make_backend();
    let before = backend.state_version.load(Ordering::Acquire);

    backend.publish_compilation(|builder| {
        builder.documents_mut();
    });
    assert_eq!(
        backend.state_version.load(Ordering::Acquire),
        before,
        "an overlay-only swap (open-document map) must not bump state_version",
    );

    backend.publish_compilation(|builder| {
        builder.workspace_index_mut();
    });
    assert_eq!(
        backend.state_version.load(Ordering::Acquire),
        before + 1,
        "a view-relevant swap must bump state_version exactly once",
    );
}

fn dot_completion_params(uri: &Url, line: u32, character: u32) -> CompletionParams {
    CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
            trigger_character: Some(".".to_string()),
        }),
    }
}

#[test]
fn completion_reflects_queued_edit_instead_of_bailing() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);

    let uri: Url = "file:///queued_completion.ws".parse().unwrap();
    backend._did_open(open_params(
        &uri,
        concat!(
            "class CExample {\n",
            "    public function DoThing() : void {}\n",
            "}\n",
            "function Test() {\n",
            "    var e : CExample;\n",
            "    e\n",
            "}\n",
        ),
    ));
    backend._did_change(change_params(&uri, 2, (5, 5), (5, 5), "."));

    let snap = backend.snapshot();
    assert!(
        backend.pending_target_for(&uri).unwrap() > snap.documents.get(&uri).unwrap().parse_version,
        "the dot edit must still be queued for this test to be meaningful",
    );

    let resp = backend
        ._completion(dot_completion_params(&uri, 5, 6))
        .expect("completion must succeed against the queued text, not bail")
        .expect("a member access must produce completions");
    let labels: Vec<String> = match resp {
        CompletionResponse::Array(items) => items.into_iter().map(|i| i.label).collect(),
        CompletionResponse::List(list) => list.items.into_iter().map(|i| i.label).collect(),
    };
    assert!(
        labels.contains(&"DoThing".to_string()),
        "completion must resolve members from the queued dot edit, got {labels:?}",
    );
}

fn workspace_diagnostic_params() -> WorkspaceDiagnosticParams {
    WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: Vec::new(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

// Asserts the pull parks (not busy-loops on ServerCancelled): Pending until index-ready notify.
async fn ready_after_notify<F>(backend: &Backend, mut fut: std::pin::Pin<Box<F>>) -> F::Output
where
    F: std::future::Future,
{
    use std::future::Future;
    use std::task::Poll;

    let pending = std::future::poll_fn(|cx| Poll::Ready(Future::poll(fut.as_mut(), cx))).await;
    assert!(
        matches!(pending, Poll::Pending),
        "pull must park while base scripts are still indexing",
    );
    backend.initial_index_done.store(true, Ordering::Release);
    backend.index_ready_notify.notify_waiters();
    fut.await
}

#[tokio::test]
async fn document_diagnostic_parks_until_initial_index() {
    use async_lsp::LanguageServer;

    let mut backend = workspace_scope_backend_indexing_pending();
    let uri: Url = "file:///diag_gate.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "class CGate {}\n"));

    let fut = Box::pin(backend.document_diagnostic(document_diagnostic_params(&uri)));
    let report = ready_after_notify(&backend, fut)
        .await
        .expect("post-index document pull must produce a report");
    assert!(
        matches!(report, DocumentDiagnosticReportResult::Report(_)),
        "once the index is ready the parked pull returns a report",
    );
}

#[tokio::test]
async fn workspace_diagnostic_parks_until_initial_index() {
    use async_lsp::LanguageServer;

    let mut backend = workspace_scope_backend_indexing_pending();
    let uri: Url = "file:///ws_diag_gate.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "class CGate {}\n"));

    let fut = Box::pin(backend.workspace_diagnostic(workspace_diagnostic_params()));
    let report = ready_after_notify(&backend, fut)
        .await
        .expect("post-index workspace pull must produce a report");
    assert!(
        matches!(report, WorkspaceDiagnosticReportResult::Report(_)),
        "once the index is ready the parked pull returns a report",
    );
}

#[test]
fn formatting_unrelated_uri_unaffected_by_pending_edit_elsewhere() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);

    let main: Url = "file:///main_fmt.ws".parse().unwrap();
    let utils: Url = "file:///utils_fmt.ws".parse().unwrap();
    backend._did_open(open_params(&main, "function Foo() {}\n"));
    backend._did_open(open_params(&utils, "function Bar() {}\n"));

    backend._did_change(change_params(&main, 2, (0, 9), (0, 12), "Renamed"));

    let result = backend._formatting(formatting_params(&utils));
    assert!(
        result.is_ok(),
        "an edit to main_fmt.ws must not CONTENT_MODIFIED a format on utils_fmt.ws",
    );
}
