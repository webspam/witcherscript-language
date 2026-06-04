use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use async_lsp::ResponseError;
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionProviderCapability, CodeLensOptions,
    CompletionOptions, DiagnosticOptions, DiagnosticServerCapabilities,
    DidChangeConfigurationParams, FileOperationFilter, FileOperationPattern,
    FileOperationPatternKind, FileOperationRegistrationOptions, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, OneOf, RenameOptions,
    SemanticTokenModifier, SemanticTokenType, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensOptions, SemanticTokensServerCapabilities, ServerCapabilities,
    SignatureHelpOptions, TextDocumentSyncCapability, TextDocumentSyncKind,
    TypeDefinitionProviderCapability, WorkDoneProgressOptions,
    WorkspaceFileOperationsServerCapabilities, WorkspaceFoldersServerCapabilities,
    WorkspaceServerCapabilities,
};
use tracing::{info, trace};
use witcherscript_language::formatter::AnnotationPlacement;
use witcherscript_language::semantic_tokens::{TOKEN_MODIFIERS, TOKEN_TYPES};

use crate::backend::Backend;
use crate::config::DiagnosticsScope;
use crate::convert::workspace_roots;
use crate::logging::{level_from_str, level_to_u8};

type Result<T> = std::result::Result<T, ResponseError>;

fn ws_file_operations_capabilities() -> WorkspaceFileOperationsServerCapabilities {
    let registration = || {
        Some(FileOperationRegistrationOptions {
            filters: vec![FileOperationFilter {
                scheme: Some("file".to_string()),
                pattern: FileOperationPattern {
                    glob: "**/*.ws".to_string(),
                    matches: Some(FileOperationPatternKind::File),
                    options: None,
                },
            }],
        })
    };
    WorkspaceFileOperationsServerCapabilities {
        did_create: registration(),
        did_rename: registration(),
        did_delete: registration(),
        ..WorkspaceFileOperationsServerCapabilities::default()
    }
}

impl Backend {
    pub(crate) async fn _initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let started_at = Instant::now();
        trace!(op = "initialize", "start");
        // Capture base scripts path from initializationOptions if provided.
        // workspace/configuration is pulled after initialized(), but this ensures
        // we have a value even before that round-trip completes.
        if let Some(opts) = &params.initialization_options {
            if let Some(p) = opts.get("gameDirectory").and_then(|v| v.as_str()) {
                if !p.is_empty() {
                    *self.base_scripts_path.lock() = Some(PathBuf::from(p));
                }
            }
            if let Some(arr) = opts
                .get("additionalScriptDirectories")
                .and_then(|v| v.as_array())
            {
                let dirs: Vec<PathBuf> = arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect();
                *self.additional_script_dirs.lock() = dirs;
            }
            if let Some(arr) = opts
                .get("legacyScriptDirectories")
                .and_then(|v| v.as_array())
            {
                let dirs: Vec<PathBuf> = arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect();
                *self.legacy_script_dirs.lock() = dirs;
            }
            let mut cfg = (**self.config.load()).clone();
            if let Some(b) = opts
                .get("autoLoadModSharedImports")
                .and_then(|v| v.as_bool())
            {
                cfg.auto_load_mod_shared_imports = b;
            }
            if let Some(b) = opts.get("detectProjectManifests").and_then(|v| v.as_bool()) {
                cfg.auto_detect_project_manifests = b;
            }
            if let Some(diag) = opts.get("diagnostics") {
                if let Some(s) = diag.get("scope").and_then(|v| v.as_str()) {
                    cfg.diagnostics_scope = DiagnosticsScope::from_setting(s);
                }
            }
            if let Some(b) = opts
                .get("codeLens")
                .and_then(|v| v.get("overriddenSymbols"))
                .and_then(|v| v.as_bool())
            {
                cfg.code_lens_overridden_symbols = b;
            }
            if let Some(b) = opts
                .get("codeLens")
                .and_then(|v| v.get("references"))
                .and_then(|v| v.as_bool())
            {
                cfg.code_lens_references = b;
            }
            if let Some(level_str) = opts.get("logLevel").and_then(|v| v.as_str()) {
                cfg.log_level = level_to_u8(level_from_str(level_str));
            }
            if let Some(formatter) = opts.get("formatter") {
                if let Some(limit) = formatter.get("lineLimit").and_then(|v| v.as_u64()) {
                    cfg.formatter_line_limit = limit as u32;
                }
                if let Some(compact) = formatter.get("compactColon").and_then(|v| v.as_bool()) {
                    cfg.formatter_compact_colon = compact;
                }
                if let Some(align) = formatter.get("alignMemberColons").and_then(|v| v.as_bool()) {
                    cfg.formatter_align_member_colons = align;
                }
                if let Some(placement) = formatter
                    .get("annotationPlacement")
                    .and_then(|v| v.as_str())
                {
                    cfg.formatter_annotation_placement =
                        AnnotationPlacement::from_setting(placement);
                }
                if let Some(placement) = formatter.get("defaultPlacement").and_then(|v| v.as_str())
                {
                    cfg.formatter_default_placement = AnnotationPlacement::from_setting(placement);
                }
            }
            self.config.store(Arc::new(cfg));
        }

        let supports_pull = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|td| td.diagnostic.as_ref())
            .is_some();
        self.client_supports_pull_diagnostics
            .store(supports_pull, Ordering::Release);

        let supports_code_lens_refresh = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.code_lens.as_ref())
            .and_then(|cl| cl.refresh_support)
            .unwrap_or(false);
        self.client_supports_code_lens_refresh
            .store(supports_code_lens_refresh, Ordering::Release);

        let supports_semantic_tokens_refresh = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.semantic_tokens.as_ref())
            .and_then(|st| st.refresh_support)
            .unwrap_or(false);
        self.client_supports_semantic_tokens_refresh
            .store(supports_semantic_tokens_refresh, Ordering::Release);

        let roots = workspace_roots(params);
        let game_dir = self.base_scripts_path.lock().clone();
        info!(roots = ?roots, game_dir = ?game_dir, supports_pull, "LSP initialize");
        *self.workspace_roots.lock() = roots;

        let result = Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        ":".to_string(),
                        "@".to_string(),
                    ]),
                    ..CompletionOptions::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![",".to_string()]),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                definition_provider: Some(OneOf::Left(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: TOKEN_TYPES
                                    .iter()
                                    .map(|s| SemanticTokenType::new(s))
                                    .collect(),
                                token_modifiers: TOKEN_MODIFIERS
                                    .iter()
                                    .map(|s| SemanticTokenModifier::new(s))
                                    .collect(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            ..SemanticTokensOptions::default()
                        },
                    ),
                ),
                document_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                        resolve_provider: None,
                    },
                )),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(true),
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: Some(ws_file_operations_capabilities()),
                }),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        identifier: Some("witcherscript".to_string()),
                        inter_file_dependencies: true,
                        workspace_diagnostics: true,
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    },
                )),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        });
        trace!(
            op = "initialize",
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _initialized(&self, _: InitializedParams) {
        let started_at = Instant::now();
        trace!(op = "initialized", "start");
        self.spawn_edit_writer();
        self.fetch_config().await;
        self.index_workspace().await;
        self.refresh_manifest_legacy_dirs();
        self.register_file_watchers().await;
        self.index_base_scripts().await;
        self.initial_index_done.store(true, Ordering::Release);
        self.index_ready_notify.notify_waiters();
        self.request_semantic_tokens_refresh();
        self.notify_diagnostics_changed();
        // The client's first codeLens request raced ahead of index_base_scripts above; re-pull now that override maps exist.
        self.request_code_lens_refresh();
        trace!(
            op = "initialized",
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    pub(crate) async fn _did_change_configuration(&self, _: DidChangeConfigurationParams) {
        let started_at = Instant::now();
        trace!(op = "did_change_configuration", "start");
        let initial_done = self.initial_index_done.load(Ordering::Acquire);
        let change = self.fetch_config().await;
        if !initial_done {
            trace!(
                op = "did_change_configuration",
                elapsed_us = started_at.elapsed().as_micros(),
                reason = "startup_echo",
                "complete",
            );
            return;
        }
        if change.needs_reindex {
            self.index_workspace().await;
            self.refresh_manifest_legacy_dirs();
            self.index_base_scripts().await;
            self.reindex_open_documents();
        }
        if change.needs_reindex || change.diagnostics_changed {
            self.notify_diagnostics_changed();
        }
        if change.code_lens_changed || change.needs_reindex {
            self.request_semantic_tokens_refresh();
            self.request_code_lens_refresh();
        }
        self.publish_file_scope_status();
        trace!(
            op = "did_change_configuration",
            elapsed_us = started_at.elapsed().as_micros(),
            reindexed = change.needs_reindex,
            "complete",
        );
    }
}
