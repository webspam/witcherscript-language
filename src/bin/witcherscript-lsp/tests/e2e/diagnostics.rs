use lsp_types::request::{DocumentDiagnosticRequest, WorkspaceDiagnosticRequest};
use lsp_types::{
    DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    PartialResultParams, PreviousResultId, TextDocumentIdentifier, Url, WorkDoneProgressParams,
    WorkspaceDiagnosticParams, WorkspaceDiagnosticReportResult, WorkspaceDocumentDiagnosticReport,
};

use super::harness::LspClient;

#[tokio::test]
async fn diagnostics_emitted_for_unclosed_class() {
    let uri: Url = "file:///bad.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;

    let diags = client.pull_diagnostics(&uri).await;
    assert!(
        !diags.is_empty(),
        "expected at least one diagnostic for unclosed class body"
    );
}

#[tokio::test]
async fn diagnostics_clear_after_fixing_source() {
    let uri: Url = "file:///fix.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;
    let bad = client.pull_diagnostics(&uri).await;
    assert!(!bad.is_empty(), "broken source should report diagnostics");

    client.change_full(&uri, 2, "class Foo {}\n").await;
    let good = client.pull_diagnostics(&uri).await;
    assert!(
        good.is_empty(),
        "fixed source should clear diagnostics, got {good:?}"
    );
}

// LSP 3.17 pull diagnostics: client requests textDocument/diagnostic with a previousResultId.
// First request returns Full + result_id. Same id on a follow-up returns Unchanged. After an
// edit, the same id should be stale, and the third response goes back to Full.
#[tokio::test]
async fn pull_diagnostics_returns_full_then_unchanged_then_full_after_edit() {
    let uri: Url = "file:///pull.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;

    let request = |previous: Option<String>| DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        identifier: None,
        previous_result_id: previous,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    // Workspace state (e.g. legacy_db_generation) may still be settling after initialized; poll
    // until the result_id is stable, then we know an Unchanged reply is safe to expect.
    let stable_id = {
        let mut prev: Option<String> = None;
        loop {
            let report = client
                .request_when_ready::<DocumentDiagnosticRequest>(request(prev.clone()))
                .await;
            let (items, id) = match report {
                DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => (
                    Some(full.full_document_diagnostic_report.items),
                    full.full_document_diagnostic_report.result_id,
                ),
                DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(u)) => {
                    break u.unchanged_document_diagnostic_report.result_id;
                }
                other => panic!("unexpected partial report: {other:?}"),
            };
            if let Some(items) = items {
                assert!(!items.is_empty(), "broken source must have diagnostics");
            }
            prev = Some(id.expect("Full report must carry a result_id"));
        }
    };

    client.change_full(&uri, 2, "class Foo {}\n").await;
    let third = client
        .request_when_ready::<DocumentDiagnosticRequest>(request(Some(stable_id)))
        .await;
    match third {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            assert!(
                full.full_document_diagnostic_report.items.is_empty(),
                "fixed source should have no diagnostics, got {:?}",
                full.full_document_diagnostic_report.items
            );
        }
        other => panic!("expected Full report after edit, got {other:?}"),
    }
}

#[tokio::test]
async fn pull_diagnostics_capability_is_advertised() {
    let client = LspClient::spawn().await;
    let caps = client.server_capabilities();
    assert!(
        caps.diagnostic_provider.is_some(),
        "server must advertise textDocument/diagnostic support"
    );
}

#[tokio::test]
async fn closing_a_file_in_open_files_scope_clears_its_client_side_diagnostics() {
    let uri: Url = "file:///scoped.ws".parse().unwrap();
    let mut client = LspClient::spawn_open_files_scope().await;
    client.open(&uri, "class Foo {\n").await;
    let diags = client.pull_diagnostics(&uri).await;
    assert!(
        !diags.is_empty(),
        "open broken file should report diagnostics"
    );

    let workspace = client
        .request_when_ready::<WorkspaceDiagnosticRequest>(WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: Vec::new(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await;
    let WorkspaceDiagnosticReportResult::Report(open_report) = workspace else {
        panic!("server must return a complete workspace report, not a partial");
    };
    let prior_result_id = open_report
        .items
        .iter()
        .find_map(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) if full.uri == uri => {
                full.full_document_diagnostic_report.result_id.clone()
            }
            _ => None,
        })
        .expect("open broken file must appear as Full with a result_id");

    client.close(&uri).await;
    let after_close = client
        .request_when_ready::<WorkspaceDiagnosticRequest>(WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: vec![PreviousResultId {
                uri: uri.clone(),
                value: prior_result_id,
            }],
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await;
    let WorkspaceDiagnosticReportResult::Report(closed_report) = after_close else {
        panic!("server must return a complete workspace report after close, not a partial");
    };
    let entry = closed_report
        .items
        .iter()
        .find(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) => full.uri == uri,
            WorkspaceDocumentDiagnosticReport::Unchanged(unchanged) => unchanged.uri == uri,
        })
        .expect("closed file the client still tracks must be explicitly cleared, not omitted");
    match entry {
        WorkspaceDocumentDiagnosticReport::Full(full) => assert!(
            full.full_document_diagnostic_report.items.is_empty(),
            "client-side clear requires empty items, got {full:?}",
        ),
        WorkspaceDocumentDiagnosticReport::Unchanged(_) => {
            panic!("a file that left the diagnosed set must not return Unchanged")
        }
    }
}

// Pre-fix, pulls parked behind a blocked initial index; enough of them filled the request cap and deadlocked the server.
#[tokio::test]
async fn pull_diagnostics_answers_empty_while_initial_index_is_blocked() {
    let uri: Url = "file:///early.ws".parse().unwrap();
    let mut client = LspClient::spawn_with_held_config().await;
    client.open(&uri, "class Foo {\n").await;

    let report = client
        .request::<DocumentDiagnosticRequest>(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await;
    match report {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            assert!(
                full.full_document_diagnostic_report.items.is_empty(),
                "pre-index pull must answer empty, got {:?}",
                full.full_document_diagnostic_report.items
            );
        }
        other => panic!("pre-index pull must answer a Full report, got {other:?}"),
    }

    client.wait_until_indexed().await;
    let diags = client.pull_diagnostics(&uri).await;
    assert!(
        !diags.is_empty(),
        "broken file must report diagnostics once indexing completes"
    );
}

#[tokio::test]
async fn workspace_diagnostics_answer_empty_while_initial_index_is_blocked() {
    let mut client = LspClient::spawn_with_held_config().await;
    let report = client
        .request::<WorkspaceDiagnosticRequest>(WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: Vec::new(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await;
    let WorkspaceDiagnosticReportResult::Report(report) = report else {
        panic!("pre-index workspace pull must answer a complete report");
    };
    assert!(
        report.items.is_empty(),
        "pre-index workspace pull must answer empty, got {:?}",
        report.items
    );
}

#[tokio::test]
async fn workspace_diagnostic_advertises_workspace_pull_support() {
    let client = LspClient::spawn().await;
    let opts = match client
        .server_capabilities()
        .diagnostic_provider
        .as_ref()
        .expect("diagnostic_provider must be advertised")
    {
        lsp_types::DiagnosticServerCapabilities::Options(opts) => opts,
        lsp_types::DiagnosticServerCapabilities::RegistrationOptions(opts) => {
            &opts.diagnostic_options
        }
    };
    assert!(
        opts.workspace_diagnostics,
        "workspace_diagnostics must be advertised so clients pull for unopened files",
    );
}
