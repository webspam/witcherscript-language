use std::sync::atomic::Ordering;
use std::time::Instant;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::{
    CodeActionParams, CodeActionResponse, CodeActionTriggerKind, CodeLens, CodeLensParams, Command,
    DiagnosticServerCancellationData, DocumentDiagnosticParams, DocumentDiagnosticReport,
    DocumentDiagnosticReportResult, DocumentFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, FullDocumentDiagnosticReport, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, InlayHint, InlayHintParams,
    Location, MarkupContent, MarkupKind, Position, RelatedFullDocumentDiagnosticReport,
    RelatedUnchangedDocumentDiagnosticReport, SemanticToken, SemanticTokens, SemanticTokensParams,
    SemanticTokensResult, SignatureHelp, SignatureHelpParams, TextEdit,
    UnchangedDocumentDiagnosticReport, Url, WorkspaceDiagnosticParams,
    WorkspaceDiagnosticReportResult,
};

use crate::config::DiagnosticsScope;
use tracing::{trace, warn};
use witcherscript_language::builtins::builtin_source;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::formatter::{format_document, FormatOptions};
use witcherscript_language::resolve::{
    inlay_hints, overridden_top_level, resolve_all_definitions, resolve_definition,
    resolve_type_definition, signature_help, OverriddenSymbol,
};
use witcherscript_language::semantic_tokens::collect_semantic_tokens_cancellable;
use witcherscript_language::symbols::{Symbol, SymbolKind};

use crate::backend::{diagnostics_document_for, Backend};
use crate::convert::{
    base_script_conflict_code_actions, document_symbols, hover_markdown, inlay_hint, lsp_range,
    refactor_code_actions, signature_help_response, source_position, source_range,
};
use crate::diagnostics_publish::publish_url;

type Result<T> = std::result::Result<T, ResponseError>;

const GO_TO_BASE_COMMAND: &str = "witcherscript.goToBaseDefinition";
const GO_TO_BASE_TITLE: &str = "game definition";
const SHOW_REFERENCES_COMMAND: &str = "witcherscript.showReferences";

// Identifies the declaration a reference-count lens belongs to so phase 2 can re-resolve it.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ReferenceLensData {
    pub(crate) uri: Url,
    pub(crate) position: Position,
}

// Custom command, not a built-in: VS Code built-ins reject raw JSON args, so the extension wrapper reconstructs vscode types from this Location.
fn base_definition_lens(overridden: OverriddenSymbol) -> Option<CodeLens> {
    let uri = match Url::parse(&overridden.base.uri) {
        Ok(uri) => uri,
        Err(err) => {
            warn!(uri = %overridden.base.uri, %err, "base symbol uri failed to parse; skipping lens");
            return None;
        }
    };
    let target = Location {
        uri,
        range: lsp_range(overridden.base.symbol.selection_range),
    };
    let argument = serde_json::to_value(target).expect("Location always serializes");
    Some(CodeLens {
        range: lsp_range(overridden.range),
        command: Some(Command {
            title: GO_TO_BASE_TITLE.to_string(),
            command: GO_TO_BASE_COMMAND.to_string(),
            arguments: Some(vec![argument]),
        }),
        data: None,
    })
}

fn symbol_eligible_for_reference_lens(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Function
            | SymbolKind::State
            | SymbolKind::Method
            | SymbolKind::Event
    )
}

fn reference_lens(symbol: &Symbol, uri: &Url) -> CodeLens {
    let range = lsp_range(symbol.selection_range);
    let data = ReferenceLensData {
        uri: uri.clone(),
        position: range.start,
    };
    CodeLens {
        range,
        command: None,
        data: Some(serde_json::to_value(data).expect("ReferenceLensData always serializes")),
    }
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
        let scope = self.config.load().diagnostics_scope;
        if matches!(scope, DiagnosticsScope::None) {
            trace!(
                op = "document_diagnostic",
                uri = %uri,
                elapsed_us = started_at.elapsed().as_micros(),
                reason = "scope_none",
                "complete",
            );
            return empty_full();
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

    pub(crate) fn _workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        let started_at = Instant::now();
        trace!(op = "workspace_diagnostic", "start");
        let version = self.state_version.load(Ordering::Acquire);
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

    pub(crate) fn _code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "code_action", uri = %uri, "start");
        let roots = self.workspace_roots.load_full();
        let mut actions = base_script_conflict_code_actions(&params.context.diagnostics, &roots);
        // An Automatic trigger is the editor requesting code actions on its own, not the user asking
        if params.context.trigger_kind != Some(CodeActionTriggerKind::AUTOMATIC) {
            actions.extend(self.refactor_actions(&uri, params.range.start));
        }
        trace!(
            op = "code_action",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        Ok((!actions.is_empty()).then_some(actions))
    }

    fn refactor_actions(&self, uri: &Url, position: Position) -> CodeActionResponse {
        let Some(document_arc) = self.latest_parsed_document(uri) else {
            return Vec::new();
        };
        let document = document_arc.as_ref();
        let Some(cursor) = document
            .line_index
            .position_to_byte(&document.source, source_position(position))
        else {
            return Vec::new();
        };
        let cfg = self.config.load();
        let options = self.format_options(!cfg.editor_insert_spaces, cfg.editor_tab_size);
        refactor_code_actions(uri, document, cursor, options)
    }

    fn format_options(&self, use_tabs: bool, tab_size: u32) -> FormatOptions {
        let cfg = self.config.load();
        FormatOptions {
            tab_size,
            use_tabs,
            line_limit: cfg.formatter_line_limit,
            compact_colon: cfg.formatter_compact_colon,
            align_member_colons: cfg.formatter_align_member_colons,
            annotation_placement: cfg.formatter_annotation_placement,
            default_placement: cfg.formatter_default_placement,
        }
    }

    pub(crate) fn _definition(
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
            let definitions = resolve_all_definitions(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
            );

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

    pub(crate) fn _type_definition(
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

            let Some(type_def) = resolve_type_definition(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
            ) else {
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

    pub(crate) fn _hover(&self, params: HoverParams) -> Result<Option<Hover>> {
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
            let Some(definition) = resolve_definition(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
            ) else {
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

    pub(crate) fn _signature_help(
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
                &canonical_uri(&uri),
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

    pub(crate) fn _document_symbol(
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

    pub(crate) fn _code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        trace!(op = "code_lens", uri = %uri, "start");
        let result = 'body: {
            let cfg = self.config.load();
            let want_overrides = cfg.code_lens_overridden_symbols;
            let want_references = cfg.code_lens_references;
            if !want_overrides && !want_references {
                trace!(op = "code_lens", uri = %uri, reason = "feature_disabled", "skip");
                break 'body Ok(None);
            }
            let snap = self.snapshot();
            let Some(document) = snap.documents.get(&uri).cloned() else {
                trace!(op = "code_lens", uri = %uri, reason = "no_open_document", "skip");
                break 'body Ok(None);
            };
            let mut lenses: Vec<CodeLens> = Vec::new();
            // References first so it keeps a fixed left position; the optional game-def lens renders to its right.
            if want_references {
                lenses.extend(
                    document
                        .symbols
                        .all()
                        .iter()
                        .filter(|s| symbol_eligible_for_reference_lens(s))
                        .map(|s| reference_lens(s, &uri)),
                );
            }
            if want_overrides && self.replaces_base_script(&uri) {
                lenses.extend(
                    overridden_top_level(document.symbols.all(), &snap.base_scripts_index)
                        .into_iter()
                        .filter_map(base_definition_lens),
                );
            }
            trace!(
                op = "code_lens",
                uri = %uri,
                base_docs = snap.base_scripts_index.documents().count(),
                lenses = lenses.len(),
                "computed",
            );
            Ok((!lenses.is_empty()).then_some(lenses))
        };
        trace!(
            op = "code_lens",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _code_lens_resolve(&self, mut lens: CodeLens) -> Result<CodeLens> {
        // Game-definition lenses arrive fully built (command set, no data); pass them through.
        let Some(data) = lens.data.take() else {
            return Ok(lens);
        };
        let ReferenceLensData { uri, position } = serde_json::from_value(data).map_err(|err| {
            ResponseError::new(
                ErrorCode::INVALID_PARAMS,
                format!("malformed reference code-lens data: {err}"),
            )
        })?;
        self.await_initial_index().await;
        self.spawn_compute(move |b| b._code_lens_resolve_blocking(lens, uri, position))
            .await
    }

    pub(crate) fn _code_lens_resolve_blocking(
        &self,
        mut lens: CodeLens,
        uri: Url,
        position: Position,
    ) -> Result<CodeLens> {
        let started_at = Instant::now();
        trace!(op = "code_lens_resolve", uri = %uri, "start");
        let locations = self
            .reference_locations(&uri, position, false)
            .unwrap_or_default();
        let count = locations.len();
        let title = if count == 1 {
            "1 reference".to_string()
        } else {
            format!("{count} references")
        };
        let arguments = vec![
            serde_json::to_value(&uri).expect("Url always serializes"),
            serde_json::to_value(position).expect("Position always serializes"),
            serde_json::to_value(&locations).expect("Locations always serialize"),
        ];
        lens.command = Some(Command {
            title,
            command: SHOW_REFERENCES_COMMAND.to_string(),
            arguments: Some(arguments),
        });
        trace!(
            op = "code_lens_resolve",
            uri = %uri,
            count,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        Ok(lens)
    }

    pub(crate) fn _semantic_tokens_full(
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
            let version = self.state_version.load(Ordering::Acquire);
            let state_version = self.state_version.clone();
            let should_continue = || state_version.load(Ordering::Acquire) == version;
            let Some(data) = collect_semantic_tokens_cancellable(
                &canonical_uri(&uri),
                document,
                &db,
                &should_continue,
            ) else {
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

    pub(crate) fn _inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        trace!(op = "inlay_hint", uri = %uri, "start");
        let result = 'body: {
            if !self.config.load().inlay_hints {
                break 'body Ok(None);
            }
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let target = self.pending_target_for(&uri).unwrap_or(0);
            if target > document.parse_version {
                break 'body Err(ResponseError::new(
                    ErrorCode::CONTENT_MODIFIED,
                    "document edited while computing inlay hints",
                ));
            }
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let version = self.state_version.load(Ordering::Acquire);
            let state_version = self.state_version.clone();
            let should_continue = || state_version.load(Ordering::Acquire) == version;
            let range = source_range(
                source_position(params.range.start),
                source_position(params.range.end),
            );
            let Some(infos) =
                inlay_hints(&canonical_uri(&uri), document, &db, range, &should_continue)
            else {
                break 'body Err(ResponseError::new(
                    ErrorCode::CONTENT_MODIFIED,
                    "document changed while computing inlay hints",
                ));
            };
            Ok(Some(infos.into_iter().map(inlay_hint).collect()))
        };
        trace!(
            op = "inlay_hint",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) fn _formatting(
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

            // Include a queued edit: clients don't retry formatting on CONTENT_MODIFIED, so
            // bailing would silently apply nothing instead of formatting the just-typed text.
            let Some(document_arc) = self.latest_parsed_document(&uri) else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();

            let formatted = format_document(
                document.tree.root_node(),
                &document.source,
                self.format_options(use_tabs, tab_size),
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
