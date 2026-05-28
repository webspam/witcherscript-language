use lsp_types::request::DocumentDiagnosticRequest;
use lsp_types::{
    DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    PartialResultParams, TextDocumentIdentifier, Url, WorkDoneProgressParams,
};

use super::harness::LspClient;

#[tokio::test]
async fn diagnostics_emitted_for_unclosed_class() {
    let uri: Url = "file:///bad.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;

    let diags = client.wait_diagnostics(&uri).await;
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
    let bad = client.wait_diagnostics(&uri).await;
    assert!(!bad.is_empty(), "broken source should report diagnostics");

    client.change_full(&uri, 2, "class Foo {}\n").await;
    let good = client.wait_diagnostics(&uri).await;
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
                .request::<DocumentDiagnosticRequest>(request(prev.clone()))
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
        .request::<DocumentDiagnosticRequest>(request(Some(stable_id)))
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
async fn closing_a_file_in_open_files_scope_clears_its_diagnostics() {
    let uri: Url = "file:///scoped.ws".parse().unwrap();
    let mut client = LspClient::spawn_open_files_scope().await;
    client.open(&uri, "class Foo {\n").await;
    let diags = client.wait_diagnostics(&uri).await;
    assert!(
        !diags.is_empty(),
        "open broken file should report diagnostics"
    );

    client.close(&uri).await;
    let cleared = client.wait_diagnostics(&uri).await;
    assert!(
        cleared.is_empty(),
        "closing in open-files scope must clear diagnostics, got {cleared:?}"
    );
}
