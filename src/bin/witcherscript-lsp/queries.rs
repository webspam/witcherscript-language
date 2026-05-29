use std::sync::atomic::Ordering;
use std::time::Instant;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::{
    CodeActionParams, CodeActionResponse, DiagnosticServerCancellationData,
    DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    DocumentFormattingParams, DocumentSymbolParams, DocumentSymbolResponse,
    FullDocumentDiagnosticReport, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverContents, HoverParams, Location, MarkupContent, MarkupKind,
    RelatedFullDocumentDiagnosticReport,
    RelatedUnchangedDocumentDiagnosticReport, SemanticToken, SemanticTokens, SemanticTokensParams,
    SemanticTokensResult, SignatureHelp, SignatureHelpParams, TextEdit,
    UnchangedDocumentDiagnosticReport, Url, WorkspaceDiagnosticParams,
    WorkspaceDiagnosticReportResult,
};

use crate::config::DiagnosticsScope;
use tracing::trace;
use witcherscript_language::builtins::builtin_source;
use witcherscript_language::formatter::{format_document, FormatOptions};
use witcherscript_language::resolve::{
    parse_generic_type, resolve_all_definitions, resolve_definition, signature_help, Definition,
    SymbolDb,
};
use witcherscript_language::symbols::SymbolKind;
use witcherscript_language::semantic_tokens::collect_semantic_tokens_cancellable;

use crate::backend::Backend;
use crate::convert::{
    base_script_conflict_code_actions, document_symbols, hover_markdown, lsp_range,
    signature_help_response, source_position,
};
use crate::diagnostics_publish::publish_url;

type Result<T> = std::result::Result<T, ResponseError>;

impl Backend {
    pub(crate) async fn _document_diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "document_diagnostic", uri = %uri, "start");
        let empty_full = || {
            Ok(DocumentDiagnosticReportResult::Report(
                DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items: Vec::new(),
                    },
                }),
            ))
        };
        if matches!(self.config.load().diagnostics_scope, DiagnosticsScope::None) {
            trace!(
                op = "document_diagnostic",
                uri = %uri,
                elapsed_us = started_at.elapsed().as_micros(),
                reason = "scope_none",
                "complete",
            );
            return empty_full();
        }
        let version = self.diagnostic_version.load(Ordering::Acquire);
        let result = 'body: {
            let computed = {
                let snap = self.snapshot();
                let Some(document) = snap.documents.get(&uri).cloned() else {
                    break 'body empty_full();
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

    pub(crate) async fn _workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        let started_at = Instant::now();
        trace!(op = "workspace_diagnostic", "start");
        let version = self.diagnostic_version.load(Ordering::Acquire);
        let previous = params
            .previous_result_ids
            .into_iter()
            .map(|p| {
                let key = publish_url(p.uri.as_str())
                    .map(|u| u.to_string())
                    .unwrap_or_else(|| p.uri.to_string());
                (key, p.value)
            })
            .collect();
        let result = match self.compute_workspace_diagnostic_report(previous, version) {
            Some(report) => Ok(WorkspaceDiagnosticReportResult::Report(report)),
            None => Err(ResponseError::new_with_data(
                ErrorCode::SERVER_CANCELLED,
                "workspace state changed while computing diagnostics",
                serde_json::to_value(DiagnosticServerCancellationData {
                    retrigger_request: true,
                })
                .expect("DiagnosticServerCancellationData serializes"),
            )),
        };
        trace!(
            op = "workspace_diagnostic",
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "code_action", uri = %uri, "start");
        let roots = self.workspace_roots.lock().clone();
        let actions = base_script_conflict_code_actions(&params.context.diagnostics, &roots);
        trace!(
            op = "code_action",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        Ok((!actions.is_empty()).then_some(actions))
    }

    pub(crate) async fn _definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "definition", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let definitions =
                resolve_all_definitions(uri.as_str(), document, &db, source_position(position));

            let locations: Vec<Location> = definitions
                .into_iter()
                .filter_map(|definition| {
                    Url::parse(&definition.uri).ok().map(|target_uri| Location {
                        uri: target_uri,
                        range: lsp_range(definition.symbol.selection_range),
                    })
                })
                .collect();

            match locations.as_slice() {
                [] => Ok(None),
                [single] => Ok(Some(GotoDefinitionResponse::Scalar(single.clone()))),
                _ => Ok(Some(GotoDefinitionResponse::Array(locations))),
            }
        };
        trace!(
            op = "definition",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "type_definition", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();

            let Some(def) =
                resolve_definition(uri.as_str(), document, &db, source_position(position))
            else {
                break 'body Ok(None);
            };

            let Some(type_def) = type_target_for(&def, &db) else {
                break 'body Ok(None);
            };

            let Ok(target_uri) = Url::parse(&type_def.uri) else {
                break 'body Ok(None);
            };
            Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: target_uri,
                range: lsp_range(type_def.symbol.selection_range),
            })))
        };
        trace!(
            op = "type_definition",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "hover", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let Some(definition) =
                resolve_definition(uri.as_str(), document, &db, source_position(position))
            else {
                break 'body Ok(None);
            };

            Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: hover_markdown(&definition),
                }),
                range: Some(lsp_range(definition.symbol.selection_range)),
            }))
        };
        trace!(
            op = "hover",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "signature_help", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let compact_colon = self.config.load().formatter_compact_colon;

            Ok(signature_help(
                uri.as_str(),
                document,
                &db,
                source_position(position),
                compact_colon,
            )
            .map(signature_help_response))
        };
        trace!(
            op = "signature_help",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "document_symbol", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };

            Ok(Some(DocumentSymbolResponse::Nested(document_symbols(
                &document.symbols,
                None,
                uri.as_str(),
            ))))
        };
        trace!(
            op = "document_symbol",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        trace!(op = "semantic_tokens_full", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let target = self.pending_target_for(&uri).unwrap_or(0);
            if target > document.parse_version {
                break 'body Err(ResponseError::new(
                    ErrorCode::CONTENT_MODIFIED,
                    "document edited while computing semantic tokens",
                ));
            }
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let version = self.diagnostic_version.load(Ordering::Acquire);
            let diagnostic_version = self.diagnostic_version.clone();
            let should_continue = || diagnostic_version.load(Ordering::Acquire) == version;
            let Some(data) =
                collect_semantic_tokens_cancellable(uri.as_str(), document, &db, &should_continue)
            else {
                break 'body Err(ResponseError::new(
                    ErrorCode::CONTENT_MODIFIED,
                    "document changed while computing semantic tokens",
                ));
            };
            let tokens: Vec<SemanticToken> = data
                .chunks_exact(5)
                .map(|c| SemanticToken {
                    delta_line: c[0],
                    delta_start: c[1],
                    length: c[2],
                    token_type: c[3],
                    token_modifiers_bitset: c[4],
                })
                .collect();
            Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: tokens,
            })))
        };
        trace!(
            op = "semantic_tokens_full",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return Ok(None);
        }
        let started_at = Instant::now();
        trace!(op = "formatting", uri = %uri, "start");
        let result = 'body: {
            let tab_size = params.options.tab_size;
            let use_tabs = !params.options.insert_spaces;

            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();

            let cfg = self.config.load();
            let line_limit = cfg.formatter_line_limit;
            let compact_colon = cfg.formatter_compact_colon;
            let align_member_colons = cfg.formatter_align_member_colons;
            let annotation_placement = cfg.formatter_annotation_placement;

            let formatted = format_document(
                document.tree.root_node(),
                &document.source,
                FormatOptions {
                    tab_size,
                    use_tabs,
                    line_limit,
                    compact_colon,
                    align_member_colons,
                    annotation_placement,
                },
            );

            if formatted == document.source {
                break 'body Ok(Some(Vec::new()));
            }

            let full_range = lsp_range(document.line_index.byte_range_to_range(
                &document.source,
                0,
                document.source.len(),
            ));

            Ok(Some(vec![TextEdit {
                range: full_range,
                new_text: formatted,
            }]))
        };
        trace!(
            op = "formatting",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}

fn type_target_for(def: &Definition, db: &SymbolDb<'_>) -> Option<Definition> {
    match def.symbol.kind {
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum | SymbolKind::State => {
            Some(def.clone())
        }
        SymbolKind::EnumMember => {
            let owner = def.symbol.container_name.as_deref()?;
            db.find_top_level(owner)
        }
        _ => {
            let raw = def.symbol.type_annotation.as_deref()?;
            let lookup = parse_generic_type(raw).map(|(ctor, _)| ctor).unwrap_or(raw);
            db.find_top_level(lookup)
        }
    }
}
