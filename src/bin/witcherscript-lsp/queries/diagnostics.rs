use std::sync::atomic::Ordering;
use std::time::Instant;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::{
    DiagnosticServerCancellationData, DocumentDiagnosticParams, DocumentDiagnosticReport,
    DocumentDiagnosticReportResult, FullDocumentDiagnosticReport,
    RelatedFullDocumentDiagnosticReport, RelatedUnchangedDocumentDiagnosticReport,
    UnchangedDocumentDiagnosticReport, WorkspaceDiagnosticParams, WorkspaceDiagnosticReport,
    WorkspaceDiagnosticReportResult,
};

use crate::config::DiagnosticsScope;
use tracing::trace;

use crate::backend::{Backend, diagnostics_document_for};
use crate::diagnostics_publish::publish_url;

type Result<T> = std::result::Result<T, ResponseError>;

pub(crate) fn empty_full_document_report() -> DocumentDiagnosticReportResult {
    DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(
        RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: None,
                items: Vec::new(),
            },
        },
    ))
}

pub(crate) fn empty_workspace_report() -> WorkspaceDiagnosticReportResult {
    WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport { items: Vec::new() })
}

// LSP's "can't compute yet" signal; retrigger_request asks the client to re-pull once ready.
fn diagnostics_server_cancelled(message: &str) -> ResponseError {
    ResponseError::new_with_data(
        ErrorCode::SERVER_CANCELLED,
        message,
        serde_json::to_value(DiagnosticServerCancellationData {
            retrigger_request: true,
        })
        .expect("DiagnosticServerCancellationData serializes"),
    )
}

impl Backend {
    pub(crate) fn _document_diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "document_diagnostic", uri = %uri, "start");
        // Pre-index: answer empty instead of parking; parked pulls can deadlock the main loop, and a post-index refresh re-pulls.
        if !self.initial_index_done.load(Ordering::Acquire) {
            return Ok(empty_full_document_report());
        }
        let scope = self.config.load().diagnostics_scope;
        if matches!(scope, DiagnosticsScope::None) {
            trace!(
                op = "document_diagnostic",
                uri = %uri,
                elapsed_us = started_at.elapsed().as_micros(),
                reason = "scope_none",
                "complete",
            );
            return Ok(empty_full_document_report());
        }
        let version = self.state_version.load(Ordering::Acquire);
        let whole_workspace = matches!(scope, DiagnosticsScope::Workspace);
        let result = 'body: {
            let computed = {
                let snap = self.snapshot();
                let Some(document) = diagnostics_document_for(
                    &snap.workspace_documents,
                    &snap.documents,
                    &uri,
                    whole_workspace,
                ) else {
                    break 'body Ok(empty_full_document_report());
                };
                let target = self.pending_target_for(&uri).unwrap_or(0);
                if target > document.parse_version {
                    break 'body Err(ResponseError::new(
                        ErrorCode::CONTENT_MODIFIED,
                        "document edited while computing diagnostics",
                    ));
                }
                self.compute_diagnostics_for_uri(&uri, document.as_ref(), version)
            };
            let Some((items, result_id)) = computed else {
                break 'body Err(ResponseError::new(
                    ErrorCode::CONTENT_MODIFIED,
                    "document changed while computing diagnostics",
                ));
            };
            if params.previous_result_id.as_deref() == Some(result_id.as_str()) {
                break 'body Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Unchanged(RelatedUnchangedDocumentDiagnosticReport {
                        related_documents: None,
                        unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                            result_id,
                        },
                    }),
                ));
            }
            Ok(DocumentDiagnosticReportResult::Report(
                DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: Some(result_id),
                        items,
                    },
                }),
            ))
        };
        trace!(
            op = "document_diagnostic",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) fn _workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        let started_at = Instant::now();
        trace!(op = "workspace_diagnostic", "start");
        // Pre-index: answer empty instead of parking; parked pulls can deadlock the main loop, and a post-index refresh re-pulls.
        if !self.initial_index_done.load(Ordering::Acquire) {
            return Ok(empty_workspace_report());
        }
        let version = self.state_version.load(Ordering::Acquire);
        let previous = params
            .previous_result_ids
            .into_iter()
            .map(|p| {
                let key = publish_url(p.uri.as_str())
                    .map_or_else(|| p.uri.to_string(), |u| u.to_string());
                (key, p.value)
            })
            .collect();
        let result = match self.compute_workspace_diagnostic_report(&previous, version) {
            Some(report) => Ok(WorkspaceDiagnosticReportResult::Report(report)),
            None => Err(diagnostics_server_cancelled(
                "workspace state changed while computing diagnostics",
            )),
        };
        trace!(
            op = "workspace_diagnostic",
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
