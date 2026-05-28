use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::router::Router;
use async_lsp::ClientSocket;
use lsp_types::{
    DidCloseTextDocumentParams, TextDocumentIdentifier, Url, WorkspaceDocumentDiagnosticReport,
};

use super::legacy_helpers::{write_script, LocalTempDir};
use crate::backend::Backend;
use crate::config::{Config, DiagnosticsScope};

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
