use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_lsp::ResponseError;
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionProviderCapability, CompletionOptions,
    DidChangeConfigurationParams, FileOperationFilter, FileOperationPattern,
    FileOperationPatternKind, FileOperationRegistrationOptions, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, OneOf, RenameOptions,
    SemanticTokenModifier, SemanticTokenType, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensOptions, SemanticTokensServerCapabilities, ServerCapabilities,
    SignatureHelpOptions, TextDocumentSyncCapability, TextDocumentSyncKind,
    WorkDoneProgressOptions, WorkspaceFileOperationsServerCapabilities,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use tracing::info;
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
            }
            self.config.store(Arc::new(cfg));
        }

        let roots = workspace_roots(params);
        let game_dir = self.base_scripts_path.lock().clone();
        info!(roots = ?roots, game_dir = ?game_dir, "LSP initialize");
        *self.workspace_roots.lock() = roots;

        Ok(InitializeResult {
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
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: Some(ws_file_operations_capabilities()),
                }),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    pub(crate) async fn _initialized(&self, _: InitializedParams) {
        tracing::trace!("initialized: fetching config and indexing");
        self.fetch_config().await;
        self.index_workspace().await;
        self.refresh_manifest_legacy_dirs();
        self.register_file_watchers().await;
        self.index_base_scripts().await;
        self.initial_index_done.store(true, Ordering::Release);
        self.publish_open_diagnostics();
    }

    pub(crate) async fn _did_change_configuration(&self, _: DidChangeConfigurationParams) {
        let initial_done = self.initial_index_done.load(Ordering::Acquire);
        let change = self.fetch_config().await;
        if !initial_done {
            tracing::trace!("did_change_configuration: startup echo, skipping re-index");
            return;
        }
        if change.needs_reindex {
            tracing::trace!("did_change_configuration: index-relevant config changed, re-indexing");
            self.index_workspace().await;
            self.refresh_manifest_legacy_dirs();
            self.index_base_scripts().await;
            self.reindex_open_documents();
            self.publish_open_diagnostics();
        } else {
            tracing::trace!(
                "did_change_configuration: no index-relevant config change, skipping re-index"
            );
        }
        if change.diagnostics_changed {
            tracing::trace!("did_change_configuration: diagnostics setting changed, applying");
            self.reconcile_published_diagnostics();
        }
        self.publish_file_scope_status();
    }
}
