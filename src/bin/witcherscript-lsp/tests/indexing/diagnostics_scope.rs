use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::router::Router;
use async_lsp::ClientSocket;
use lsp_types::{
    DidCloseTextDocumentParams, PartialResultParams, PreviousResultId, TextDocumentIdentifier, Url,
    WorkDoneProgressParams, WorkspaceDiagnosticParams, WorkspaceDiagnosticReportResult,
    WorkspaceDocumentDiagnosticReport,
};

use super::legacy_helpers::{write_script, LocalTempDir};
use crate::backend::Backend;
use crate::config::{Config, DiagnosticsScope};
use crate::cst_cache::{CstCacheEntry, DbFingerprint};

fn make_backend_with(scope: DiagnosticsScope) -> Backend {
    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: scope,
        ..Config::default()
    }));
    let backend = Backend::new(client, config);
    backend.initial_index_done.store(true, Ordering::Release);
    backend
}

async fn index_dir(backend: &Backend, dir: &std::path::Path) {
    *backend.workspace_roots.lock() = vec![dir.to_path_buf()];
    backend.index_workspace().await;
}

fn close_params(uri: &Url) -> DidCloseTextDocumentParams {
    DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    }
}

fn workspace_report_for(backend: &Backend, url: &Url) -> Option<WorkspaceDocumentDiagnosticReport> {
    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let report = backend
        .compute_workspace_diagnostic_report(HashMap::new(), version)
        .expect("workspace pull must not bail in tests with a stable version");
    report.items.into_iter().find(|item| match item {
        WorkspaceDocumentDiagnosticReport::Full(full) => &full.uri == url,
        WorkspaceDocumentDiagnosticReport::Unchanged(unchanged) => &unchanged.uri == url,
    })
}

fn has_items(report: &WorkspaceDocumentDiagnosticReport) -> bool {
    match report {
        WorkspaceDocumentDiagnosticReport::Full(full) => {
            !full.full_document_diagnostic_report.items.is_empty()
        }
        WorkspaceDocumentDiagnosticReport::Unchanged(_) => false,
    }
}

#[tokio::test]
async fn none_scope_workspace_pull_returns_no_items_when_client_has_no_prior_state() {
    let temp = LocalTempDir::new("ws_scope_none_empty");
    write_script(temp.path(), "Bad.ws", "class CBad {\n");

    let backend = make_backend_with(DiagnosticsScope::None);
    index_dir(&backend, temp.path()).await;

    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let report = backend
        .compute_workspace_diagnostic_report(HashMap::new(), version)
        .expect("workspace pull must not bail");
    assert!(
        report.items.is_empty(),
        "None scope with no prior client state must emit zero items, got {report:?}",
    );
}

#[tokio::test]
async fn none_scope_workspace_pull_clears_prior_client_state() {
    let temp = LocalTempDir::new("ws_scope_none_clears_prior");
    let path = write_script(temp.path(), "Bad.ws", "class CBad {\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::None);
    index_dir(&backend, temp.path()).await;

    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let mut previous = HashMap::new();
    previous.insert(url.to_string(), "prior-id".to_string());
    let report = backend
        .compute_workspace_diagnostic_report(previous, version)
        .expect("workspace pull must not bail");
    let entry = report
        .items
        .iter()
        .find(
            |item| matches!(item, WorkspaceDocumentDiagnosticReport::Full(full) if full.uri == url),
        )
        .expect("prior tracked URI must be explicitly cleared under None scope");
    match entry {
        WorkspaceDocumentDiagnosticReport::Full(full) => assert!(
            full.full_document_diagnostic_report.items.is_empty(),
            "clearing entry must carry empty items, got {full:?}",
        ),
        WorkspaceDocumentDiagnosticReport::Unchanged(_) => unreachable!(),
    }
}

#[tokio::test]
async fn workspace_mode_diagnoses_every_file_without_opening_it() {
    let temp = LocalTempDir::new("ws_scope_workspace_diagnoses_all");
    let path = write_script(temp.path(), "Bad.ws", "class CBad {\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;

    let report = workspace_report_for(&backend, &url)
        .expect("workspace scope must include the unopened broken file");
    assert!(
        has_items(&report),
        "workspace scope must diagnose an unopened broken file, got {report:?}",
    );
}

#[tokio::test]
async fn opening_unchanged_indexed_file_reuses_parse_and_skips_diagnostic_refresh() {
    let temp = LocalTempDir::new("ws_reopen_unchanged_no_refresh");
    let text = "class CExample {\n  function Foo() {}\n}\n";
    let path = write_script(temp.path(), "Example.ws", text);
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;

    let canonical = witcherscript_language::files::canonical_uri(&url).expect("canonical uri");
    let indexed_version = backend
        .snapshot()
        .workspace_documents
        .get(&canonical)
        .expect("file must be indexed on disk")
        .parse_version;

    let version_before = backend.diagnostic_version.load(Ordering::Acquire);
    backend.update_open_document(url.clone(), text.to_string());

    assert_eq!(
        backend.diagnostic_version.load(Ordering::Acquire),
        version_before,
        "opening a byte-identical indexed file must not bump diagnostic_version",
    );
    let snap = backend.snapshot();
    let open_doc = snap
        .documents
        .get(&url)
        .expect("opened file must be registered in the open overlay");
    assert_eq!(
        open_doc.parse_version, indexed_version,
        "opened file must reuse the indexed parse, not a fresh parse_version",
    );
}

#[tokio::test]
async fn body_only_reindex_keeps_subscriber_cache_entries() {
    let temp = LocalTempDir::new("ws_body_reindex_keeps_subscribers");
    write_script(
        temp.path(),
        "Base.ws",
        "class CBase {\n  function f() { var x : int; }\n}\n",
    );
    write_script(
        temp.path(),
        "Derived.ws",
        "class CDerived extends CBase {\n  var owner : CBase;\n}\n",
    );
    let base_url = Url::from_file_path(temp.path().join("Base.ws")).expect("base url");
    let derived_canonical = witcherscript_language::files::canonical_uri(
        &Url::from_file_path(temp.path().join("Derived.ws")).expect("derived url"),
    )
    .expect("derived canonical");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;

    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let _ = backend
        .compute_workspace_diagnostic_report(HashMap::new(), version)
        .expect("initial pull populates the CST cache");
    assert!(
        backend
            .cst_diag_cache
            .lock()
            .contains_key(&derived_canonical),
        "precondition: the subscriber's diagnostics must be cached after the first pull",
    );

    // Body-only edit: CBase's outward surface is unchanged, so subscribers must survive.
    backend.update_open_document(
        base_url,
        "class CBase {\n  function f() { var y : int; }\n}\n".to_string(),
    );

    assert!(
        backend
            .cst_diag_cache
            .lock()
            .contains_key(&derived_canonical),
        "a body-only reindex of CBase must not evict CDerived's cached diagnostics",
    );
}

#[tokio::test]
async fn single_file_pull_does_not_evict_other_cached_files() {
    let temp = LocalTempDir::new("ws_single_pull_keeps_cache");
    write_script(temp.path(), "A.ws", "class CA {}\n");
    write_script(temp.path(), "B.ws", "class CB {}\n");
    let a_url = Url::from_file_path(temp.path().join("A.ws")).expect("a url");
    let b_canonical = witcherscript_language::files::canonical_uri(
        &Url::from_file_path(temp.path().join("B.ws")).expect("b url"),
    )
    .expect("b canonical");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;

    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let _ = backend
        .compute_workspace_diagnostic_report(HashMap::new(), version)
        .expect("workspace pull populates the cache");
    assert_eq!(backend.cst_diag_cache.lock().len(), 2);

    let a_doc = witcherscript_language::document::parse_document("class CA {}\n".to_string())
        .expect("parse");
    let _ = backend.compute_diagnostics_for_uri(&a_url, &a_doc, version);

    assert!(
        backend.cst_diag_cache.lock().contains_key(&b_canonical),
        "a single-file pull must leave other files' cache entries intact",
    );
}

#[tokio::test]
async fn workspace_pull_prunes_cache_entries_for_vanished_files() {
    let temp = LocalTempDir::new("ws_pull_prunes_vanished");
    write_script(temp.path(), "A.ws", "class CA {}\n");
    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;

    backend.cst_diag_cache.lock().insert(
        "file:///gone.ws".to_string(),
        CstCacheEntry {
            parse_version: 0,
            db_fingerprint: DbFingerprint {
                base_surface: 0,
                env: 0,
                legacy_db_generation: 0,
            },
            diagnostics: Vec::new(),
        },
    );

    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let _ = backend
        .compute_workspace_diagnostic_report(HashMap::new(), version)
        .expect("workspace pull");

    assert!(
        !backend
            .cst_diag_cache
            .lock()
            .contains_key("file:///gone.ws"),
        "a whole-workspace pull must prune cache entries for files no longer in the set",
    );
}

#[tokio::test]
async fn open_files_mode_skips_unopened_files_but_still_indexes_symbols() {
    let temp = LocalTempDir::new("ws_scope_openfiles_skips_unopened");
    let path = write_script(temp.path(), "Bad.ws", "class CBad {\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::OpenFiles);
    index_dir(&backend, temp.path()).await;

    assert!(
        workspace_report_for(&backend, &url).is_none(),
        "open-files scope must not include an unopened file in the workspace report",
    );
    assert!(
        backend
            .snapshot()
            .workspace_index
            .documents()
            .any(|(uri, _)| uri == url.as_str()),
        "open-files scope must still index the file's symbols project-wide",
    );
}

#[tokio::test]
async fn closing_a_file_drops_the_buffer_and_reverts_to_disk() {
    let temp = LocalTempDir::new("ws_scope_close_reverts_to_disk");
    let path = write_script(temp.path(), "Good.ws", "class CGood {}\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;
    backend.update_open_document(url.clone(), "class CGood {\n".to_string());
    assert!(
        backend.snapshot().documents.contains_key(&url),
        "file must be open before the close can be exercised",
    );

    backend._did_close(close_params(&url));

    assert!(
        !backend.snapshot().documents.contains_key(&url),
        "closing a file must drop its editor buffer",
    );
    let snap = backend.snapshot();
    assert_eq!(
        snap.workspace_documents
            .get(url.as_str())
            .map(|d| d.source.as_str()),
        Some("class CGood {}\n"),
        "a closed file must revert to its on-disk content, dropping unsaved edits",
    );
}

#[tokio::test]
async fn open_files_mode_close_drops_the_file_from_the_workspace_report() {
    let temp = LocalTempDir::new("ws_scope_openfiles_close_clears");
    let path = write_script(temp.path(), "Good.ws", "class CGood {}\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::OpenFiles);
    index_dir(&backend, temp.path()).await;
    backend.update_open_document(url.clone(), "class CGood {\n".to_string());
    let open_report = workspace_report_for(&backend, &url)
        .expect("open broken file must appear in the open-files workspace report");
    assert!(
        has_items(&open_report),
        "open broken file must carry diagnostics, got {open_report:?}",
    );

    backend._did_close(close_params(&url));

    assert!(
        workspace_report_for(&backend, &url).is_none(),
        "open-files scope must drop the file from the workspace report after close",
    );
}

#[tokio::test]
async fn workspace_mode_close_keeps_the_file_in_the_workspace_report() {
    let temp = LocalTempDir::new("ws_scope_workspace_close_keeps");
    let path = write_script(temp.path(), "Bad.ws", "class CBad {\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;
    backend.update_open_document(url.clone(), "class CBad {\n".to_string());

    backend._did_close(close_params(&url));

    let report = workspace_report_for(&backend, &url)
        .expect("workspace scope must still include the file after close");
    assert!(
        has_items(&report),
        "workspace scope must keep diagnostics for closed broken files, got {report:?}",
    );
}

#[tokio::test]
async fn workspace_pull_returns_unchanged_for_open_file_when_client_echoes_emitted_result_id() {
    let temp = LocalTempDir::new("ws_scope_unchanged_open_roundtrip");
    let path = write_script(temp.path(), "Bad.ws", "class CBad {\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;
    backend.update_open_document(url.clone(), "class CBad {\n".to_string());

    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let initial = backend
        .compute_workspace_diagnostic_report(HashMap::new(), version)
        .expect("initial workspace pull must not bail");
    let (emitted_uri, emitted_result_id) = initial
        .items
        .iter()
        .find_map(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) if full.uri == url => Some((
                full.uri.clone(),
                full.full_document_diagnostic_report.result_id.clone(),
            )),
            _ => None,
        })
        .expect("initial pull must include the open broken file as Full");
    let emitted_result_id = emitted_result_id.expect("Full report must carry a result_id");

    let mut previous = HashMap::new();
    previous.insert(emitted_uri.to_string(), emitted_result_id);
    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let second = backend
        .compute_workspace_diagnostic_report(previous, version)
        .expect("second workspace pull must not bail");
    let entry = second
        .items
        .iter()
        .find(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) => full.uri == url,
            WorkspaceDocumentDiagnosticReport::Unchanged(unchanged) => unchanged.uri == url,
        })
        .expect("open file must appear in the second workspace report");
    assert!(
        matches!(entry, WorkspaceDocumentDiagnosticReport::Unchanged(_)),
        "client echoing back the URI we emitted must yield Unchanged, got {entry:?}",
    );
}

#[tokio::test]
async fn workspace_pull_explicitly_clears_files_that_left_the_diagnosed_set() {
    let temp = LocalTempDir::new("ws_scope_clear_on_leave");
    let path = write_script(temp.path(), "Bad.ws", "class CBad {\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;

    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let initial = backend
        .compute_workspace_diagnostic_report(HashMap::new(), version)
        .expect("initial workspace pull must not bail");
    let prior_result_id = initial
        .items
        .iter()
        .find_map(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) if full.uri == url => {
                full.full_document_diagnostic_report.result_id.clone()
            }
            _ => None,
        })
        .expect("initial pull must return Full with a result_id for the broken file");

    let mut cfg = (**backend.config.load()).clone();
    cfg.diagnostics_scope = DiagnosticsScope::OpenFiles;
    backend.config.store(Arc::new(cfg));
    backend.notify_diagnostics_changed();

    let version = backend.diagnostic_version.load(Ordering::Acquire);
    let mut previous = HashMap::new();
    previous.insert(url.to_string(), prior_result_id);
    let cleared = backend
        .compute_workspace_diagnostic_report(previous, version)
        .expect("workspace pull after scope narrow must not bail");
    let entry = cleared
        .items
        .iter()
        .find(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) => full.uri == url,
            WorkspaceDocumentDiagnosticReport::Unchanged(u) => u.uri == url,
        })
        .expect(
            "file that left the diagnosed set must appear as an explicit clear, not be omitted",
        );
    match entry {
        WorkspaceDocumentDiagnosticReport::Full(full) => {
            assert!(
                full.full_document_diagnostic_report.items.is_empty(),
                "clearing entry must carry empty items, got {full:?}",
            );
        }
        WorkspaceDocumentDiagnosticReport::Unchanged(_) => {
            panic!("a URI that left the diagnosed set must not return Unchanged")
        }
    }
}

// VSCode echoes URIs back URL-encoded (`file:///c%3A/...`); ours canonicalise to `file:///C:/...`.
#[cfg(windows)]
#[tokio::test]
async fn workspace_pull_matches_previous_result_ids_in_client_uri_form() {
    let temp = LocalTempDir::new("ws_canonicalize_previous");
    let path = write_script(temp.path(), "Bad.ws", "class CBad {\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;

    let empty_params = || WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: Vec::new(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let WorkspaceDiagnosticReportResult::Report(initial) = backend
        ._workspace_diagnostic(empty_params())
        .await
        .expect("initial workspace pull must not bail")
    else {
        panic!("initial workspace pull must return a complete report");
    };
    let (emitted_uri, emitted_result_id) = initial
        .items
        .iter()
        .find_map(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) if full.uri == url => Some((
                full.uri.clone(),
                full.full_document_diagnostic_report.result_id.clone(),
            )),
            _ => None,
        })
        .expect("initial pull must include the broken file as Full");
    let emitted_result_id = emitted_result_id.expect("Full report must carry a result_id");

    let client_form: Url = emitted_uri
        .to_string()
        .replacen("file:///C:/", "file:///c%3A/", 1)
        .parse()
        .expect("client-form URL parses");
    assert_ne!(
        client_form, emitted_uri,
        "test must exercise a URI form different from what we emit",
    );

    let WorkspaceDiagnosticReportResult::Report(second) = backend
        ._workspace_diagnostic(WorkspaceDiagnosticParams {
            previous_result_ids: vec![PreviousResultId {
                uri: client_form,
                value: emitted_result_id,
            }],
            ..empty_params()
        })
        .await
        .expect("second workspace pull must not bail")
    else {
        panic!("second workspace pull must return a complete report");
    };
    let entry = second
        .items
        .iter()
        .find(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) => full.uri == url,
            WorkspaceDocumentDiagnosticReport::Unchanged(unchanged) => unchanged.uri == url,
        })
        .expect("second pull must include the open file");
    assert!(
        matches!(entry, WorkspaceDocumentDiagnosticReport::Unchanged(_)),
        "previous_result_ids in client URI form must match emitted publish_key, got {entry:?}",
    );
    assert_eq!(
        second.items.len(),
        1,
        "no spurious clearings must be emitted for the URI we just echoed, got {:?}",
        second.items,
    );
}

#[tokio::test]
async fn switching_scope_retracts_then_restores_unopened_diagnostics() {
    let temp = LocalTempDir::new("ws_scope_switch_retracts_restores");
    let path = write_script(temp.path(), "Bad.ws", "class CBad {\n");
    let url = Url::from_file_path(&path).expect("path -> url");

    let backend = make_backend_with(DiagnosticsScope::Workspace);
    index_dir(&backend, temp.path()).await;
    assert!(
        workspace_report_for(&backend, &url).is_some(),
        "workspace scope must include the unopened file first",
    );

    let switch = |scope| {
        let mut cfg = (**backend.config.load()).clone();
        cfg.diagnostics_scope = scope;
        backend.config.store(Arc::new(cfg));
    };

    switch(DiagnosticsScope::OpenFiles);
    backend.notify_diagnostics_changed();
    assert!(
        workspace_report_for(&backend, &url).is_none(),
        "switching to open-files scope must drop the unopened file from the workspace report",
    );

    switch(DiagnosticsScope::Workspace);
    backend.notify_diagnostics_changed();
    assert!(
        workspace_report_for(&backend, &url).is_some(),
        "switching back to workspace scope must restore the file in the workspace report",
    );
}
