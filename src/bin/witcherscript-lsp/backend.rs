use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::{ClientSocket, ErrorCode, LanguageServer, ResponseError};
use futures::future::BoxFuture;
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams,
    CompletionResponse, Diagnostic, DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, FileOperationFilter, FileOperationPattern, FileOperationPatternKind,
    FileOperationRegistrationOptions, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, InsertTextFormat, Location, MarkupContent, MarkupKind, OneOf,
    PrepareRenameResponse, ReferenceParams, RenameOptions, RenameParams, SemanticToken,
    SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, SignatureHelp, SignatureHelpOptions,
    SignatureHelpParams, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Url, WorkDoneProgressOptions, WorkspaceEdit,
    WorkspaceFileOperationsServerCapabilities, WorkspaceFoldersServerCapabilities,
    WorkspaceServerCapabilities,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex, MutexGuard};
use tracing::{error, info, trace};
use witcherscript_language::builtins::{builtin_source, load_builtins_index};
use witcherscript_language::document::{apply_content_change, ParsedDocument};
use witcherscript_language::files::canonical_uri;
use witcherscript_language::formatter::format_document;
use witcherscript_language::line_index::LineIndex;
use witcherscript_language::resolve::{
    after_wrap_method_completions, annotation_arg_completions, annotation_name_completions,
    class_body_keyword_completions, class_header_keyword_completions, completion_members,
    expression_completions, extends_completions, find_references, resolve_all_definitions,
    resolve_definition, script_body_completions, signature_help, state_owner_completions,
    statement_completions, type_completions, AfterWrapMethodCompletions, SymbolDb, WorkspaceIndex,
    BUILTIN_TYPE_COMPLETIONS,
};
use witcherscript_language::script_env::ScriptEnvironment;
use witcherscript_language::semantic_tokens::{
    collect_semantic_tokens, TOKEN_MODIFIERS, TOKEN_TYPES,
};

use crate::config::{Config, DiagnosticsScope};
use crate::convert::{
    annotation_name_items, base_script_conflict_code_actions, builtin_type_item,
    class_body_kw_item, completion_item, document_symbols, hover_markdown, keyword_snippet_item,
    lsp_range, script_body_item, signature_help_response, source_position, source_range,
    this_super_item, type_completion_item, workspace_roots, wrap_method_snippet,
};
use crate::file_scope::{classify_file_scope, FileScope};
use crate::file_scope_status::FileScopeStatusParams;
use crate::legacy_status::LegacyScriptStatusParams;
use crate::logging::{level_from_str, level_to_u8};

type Result<T> = std::result::Result<T, ResponseError>;

pub(crate) enum DocOp {
    Open(DidOpenTextDocumentParams),
    Change(DidChangeTextDocumentParams),
    Close(DidCloseTextDocumentParams),
    WatchedFiles(DidChangeWatchedFilesParams),
    WorkspaceFolders(DidChangeWorkspaceFoldersParams),
}

// Open editor docs shadow workspace docs which shadow base docs — unsaved edits win.
// Loose files form a compilation isolated from the workspace, so a search whose
// target is loose sees base + loose docs, and a workspace search excludes loose docs.
pub(crate) fn merge_documents<'a>(
    base_docs: &'a HashMap<String, ParsedDocument>,
    workspace_docs: &'a HashMap<String, ParsedDocument>,
    open_documents: &'a HashMap<Url, ParsedDocument>,
    open_loose_uris: &HashSet<Url>,
    target_is_loose: bool,
) -> HashMap<String, &'a ParsedDocument> {
    let mut merged: HashMap<String, &ParsedDocument> = HashMap::new();
    for (uri, doc) in base_docs.iter() {
        merged.insert(uri.clone(), doc);
    }
    if !target_is_loose {
        for (uri, doc) in workspace_docs.iter() {
            merged.insert(uri.clone(), doc);
        }
    }
    for (url, doc) in open_documents.iter() {
        if open_loose_uris.contains(url) == target_is_loose {
            merged.insert(url.to_string(), doc);
        }
    }
    merged
}

// The diagnosed set excludes read-only base scripts, so it cannot reuse merge_documents.
pub(crate) fn diagnostics_document_set<'a>(
    workspace_docs: &'a HashMap<String, ParsedDocument>,
    open_documents: &'a HashMap<Url, ParsedDocument>,
    whole_workspace: bool,
) -> HashMap<String, &'a ParsedDocument> {
    let mut merged: HashMap<String, &ParsedDocument> = HashMap::new();
    if whole_workspace {
        for (uri, doc) in workspace_docs.iter() {
            merged.insert(uri.clone(), doc);
        }
    }
    for (url, doc) in open_documents.iter() {
        if let Some(canonical) = canonical_uri(url) {
            merged.remove(&canonical);
        }
        merged.insert(url.to_string(), doc);
    }
    merged
}

// Base scripts are read-only: references found inside them must never become edits,
// even when the renamed symbol's declaration lives in the workspace (e.g. an
// @wrapMethod whose target's class-body declaration sits in a base script).
pub(crate) fn rename_changes(
    refs: &[(String, witcherscript_language::line_index::SourceRange)],
    new_name: &str,
    base_docs: &HashMap<String, ParsedDocument>,
) -> HashMap<Url, Vec<TextEdit>> {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for (ref_uri, range) in refs {
        if base_docs.contains_key(ref_uri) || builtin_source(ref_uri).is_some() {
            continue;
        }
        if let Ok(url) = Url::parse(ref_uri) {
            changes.entry(url).or_default().push(TextEdit {
                range: lsp_range(*range),
                new_text: new_name.to_string(),
            });
        }
    }
    changes
}

pub(crate) fn builtin_source_response(uri: &str) -> Result<Value> {
    if uri.is_empty() {
        return Err(ResponseError::new(
            ErrorCode::INVALID_PARAMS,
            "missing `uri` parameter",
        ));
    }
    Ok(match builtin_source(uri) {
        Some(text) => json!({ "text": text }),
        None => Value::Null,
    })
}

fn uri_within_any(uri: &str, dirs: &[PathBuf]) -> bool {
    let Some(path) = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()) else {
        return false;
    };
    dirs.iter().any(|dir| path.starts_with(dir))
}

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

#[derive(Debug, Clone)]
pub(crate) struct Backend {
    pub(crate) client: ClientSocket,
    pub(crate) config: Arc<ArcSwap<Config>>,
    pub(crate) documents: Arc<Mutex<HashMap<Url, ParsedDocument>>>,
    pub(crate) published_diagnostics: Arc<Mutex<HashMap<Url, Vec<Diagnostic>>>>,
    pub(crate) workspace_index: Arc<Mutex<WorkspaceIndex>>,
    pub(crate) workspace_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>,
    pub(crate) workspace_roots: Arc<Mutex<Vec<PathBuf>>>,
    pub(crate) files_exclude: Arc<Mutex<Vec<String>>>,
    pub(crate) base_scripts_path: Arc<Mutex<Option<PathBuf>>>,
    pub(crate) additional_script_dirs: Arc<Mutex<Vec<PathBuf>>>,
    pub(crate) legacy_script_dirs: Arc<Mutex<Vec<PathBuf>>>,
    // URIs last indexed into the workspace from legacy dirs, so a vanished one can be dropped.
    pub(crate) legacy_indexed_uris: Arc<Mutex<HashSet<String>>>,
    // Keyed by canonical URI so it matches open documents under any URI spelling.
    pub(crate) legacy_replacements: Arc<Mutex<HashMap<String, String>>>,
    // Dedups sends so an unchanged status is not resent on every keystroke.
    pub(crate) sent_legacy_status: Arc<Mutex<HashMap<Url, LegacyScriptStatusParams>>>,
    pub(crate) sent_file_scope_status: Arc<Mutex<HashMap<Url, FileScopeStatusParams>>>,
    pub(crate) base_scripts_index: Arc<Mutex<WorkspaceIndex>>,
    pub(crate) base_scripts_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>,
    // Transient index for editor-open files belonging to no project root.
    pub(crate) loose_index: Arc<Mutex<WorkspaceIndex>>,
    pub(crate) builtins_index: Arc<WorkspaceIndex>,
    pub(crate) script_env: Arc<Mutex<ScriptEnvironment>>,
    pub(crate) cst_diag_cache: Arc<Mutex<HashMap<String, crate::cst_cache::CstCacheEntry>>>,
    pub(crate) initial_index_done: Arc<AtomicBool>,
    pub(crate) doc_ops_tx: mpsc::UnboundedSender<DocOp>,
}

pub(crate) struct DbHandles<'a> {
    workspace: MutexGuard<'a, WorkspaceIndex>,
    base: MutexGuard<'a, WorkspaceIndex>,
    script_env: MutexGuard<'a, ScriptEnvironment>,
    builtins: &'a WorkspaceIndex,
}

impl<'a> DbHandles<'a> {
    pub(crate) fn db(&'a self) -> SymbolDb<'a> {
        SymbolDb::new(&self.workspace, &self.base)
            .with_script_env(&self.script_env)
            .with_builtins(self.builtins)
    }

    pub(crate) fn workspace(&self) -> &WorkspaceIndex {
        &self.workspace
    }

    pub(crate) fn base(&self) -> &WorkspaceIndex {
        &self.base
    }
}

impl Backend {
    pub(crate) fn new(
        client: ClientSocket,
        config: Arc<ArcSwap<Config>>,
        doc_ops_tx: mpsc::UnboundedSender<DocOp>,
    ) -> Backend {
        Backend {
            client,
            config,
            documents: Arc::new(Mutex::new(HashMap::new())),
            published_diagnostics: Arc::new(Mutex::new(HashMap::new())),
            workspace_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            workspace_documents: Arc::new(Mutex::new(HashMap::new())),
            workspace_roots: Arc::new(Mutex::new(Vec::new())),
            files_exclude: Arc::new(Mutex::new(Vec::new())),
            base_scripts_path: Arc::new(Mutex::new(None)),
            additional_script_dirs: Arc::new(Mutex::new(Vec::new())),
            legacy_script_dirs: Arc::new(Mutex::new(Vec::new())),
            legacy_indexed_uris: Arc::new(Mutex::new(HashSet::new())),
            legacy_replacements: Arc::new(Mutex::new(HashMap::new())),
            sent_legacy_status: Arc::new(Mutex::new(HashMap::new())),
            sent_file_scope_status: Arc::new(Mutex::new(HashMap::new())),
            base_scripts_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            base_scripts_documents: Arc::new(Mutex::new(HashMap::new())),
            loose_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            builtins_index: Arc::new(load_builtins_index()),
            script_env: Arc::new(Mutex::new(ScriptEnvironment::default())),
            cst_diag_cache: Arc::new(Mutex::new(HashMap::new())),
            initial_index_done: Arc::new(AtomicBool::new(false)),
            doc_ops_tx,
        }
    }

    pub(crate) async fn file_scope_of(&self, uri: &Url) -> FileScope {
        let roots = self.workspace_roots.lock().await.clone();
        let legacy_dirs = self.effective_legacy_dirs().await;
        let game_dir = self.base_scripts_path.lock().await.clone();
        let additional = self.additional_script_dirs.lock().await.clone();
        let replacements = self.legacy_replacements.lock().await;
        classify_file_scope(
            uri,
            &roots,
            &legacy_dirs,
            &replacements,
            game_dir.as_deref(),
            &additional,
        )
    }

    pub(crate) async fn loose_open_uris(
        &self,
        documents: &HashMap<Url, ParsedDocument>,
    ) -> HashSet<Url> {
        let roots = self.workspace_roots.lock().await.clone();
        let legacy_dirs = self.effective_legacy_dirs().await;
        let game_dir = self.base_scripts_path.lock().await.clone();
        let additional = self.additional_script_dirs.lock().await.clone();
        let replacements = self.legacy_replacements.lock().await;
        documents
            .keys()
            .filter(|uri| {
                classify_file_scope(
                    uri,
                    &roots,
                    &legacy_dirs,
                    &replacements,
                    game_dir.as_deref(),
                    &additional,
                )
                .is_loose()
            })
            .cloned()
            .collect()
    }

    // A loose file resolves against loose_index in the workspace slot, isolating
    // it from the real project's symbols.
    pub(crate) async fn db_handles_for(&self, uri: &Url) -> DbHandles<'_> {
        let workspace = if self.file_scope_of(uri).await.is_loose() {
            self.loose_index.lock().await
        } else {
            self.workspace_index.lock().await
        };
        DbHandles {
            workspace,
            base: self.base_scripts_index.lock().await,
            script_env: self.script_env.lock().await,
            builtins: self.builtins_index.as_ref(),
        }
    }

    pub(crate) async fn handle_builtin_source(&self, params: Value) -> Result<Value> {
        let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
        trace!(uri = uri, "witcherscript/builtinSource request");
        builtin_source_response(uri)
    }

    pub(crate) async fn dispatch_doc_op(&self, op: DocOp) {
        match op {
            DocOp::Open(p) => self._did_open(p).await,
            DocOp::Change(p) => self._did_change(p).await,
            DocOp::Close(p) => self._did_close(p).await,
            DocOp::WatchedFiles(p) => self._did_change_watched_files(p).await,
            DocOp::WorkspaceFolders(p) => self._did_change_workspace_folders(p).await,
        }
    }

    pub(crate) fn submit_doc_op(&self, op: DocOp) {
        if let Err(send_err) = self.doc_ops_tx.send(op) {
            error!(
                error = %send_err,
                "doc op consumer is gone; edit will not be applied (LSP state may be stale)"
            );
        }
    }
}

impl LanguageServer for Backend {
    type Error = ResponseError;
    type NotifyResult = ControlFlow<async_lsp::Result<()>>;

    fn initialize(
        &mut self,
        params: InitializeParams,
    ) -> BoxFuture<'static, Result<InitializeResult>> {
        let backend = self.clone();
        Box::pin(async move { backend._initialize(params).await })
    }

    fn initialized(&mut self, params: InitializedParams) -> Self::NotifyResult {
        let backend = self.clone();
        crate::spawn_logged("initialized handler", async move {
            backend._initialized(params).await
        });
        ControlFlow::Continue(())
    }

    fn shutdown(&mut self, _: ()) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn did_open(&mut self, params: DidOpenTextDocumentParams) -> Self::NotifyResult {
        self.submit_doc_op(DocOp::Open(params));
        ControlFlow::Continue(())
    }

    fn did_change(&mut self, params: DidChangeTextDocumentParams) -> Self::NotifyResult {
        self.submit_doc_op(DocOp::Change(params));
        ControlFlow::Continue(())
    }

    fn did_close(&mut self, params: DidCloseTextDocumentParams) -> Self::NotifyResult {
        self.submit_doc_op(DocOp::Close(params));
        ControlFlow::Continue(())
    }

    fn did_change_watched_files(
        &mut self,
        params: DidChangeWatchedFilesParams,
    ) -> Self::NotifyResult {
        self.submit_doc_op(DocOp::WatchedFiles(params));
        ControlFlow::Continue(())
    }

    fn did_change_configuration(
        &mut self,
        params: DidChangeConfigurationParams,
    ) -> Self::NotifyResult {
        let backend = self.clone();
        crate::spawn_logged("did_change_configuration handler", async move {
            backend._did_change_configuration(params).await
        });
        ControlFlow::Continue(())
    }

    fn definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> BoxFuture<'static, Result<Option<GotoDefinitionResponse>>> {
        let backend = self.clone();
        Box::pin(async move { backend._definition(params).await })
    }

    fn hover(&mut self, params: HoverParams) -> BoxFuture<'static, Result<Option<Hover>>> {
        let backend = self.clone();
        Box::pin(async move { backend._hover(params).await })
    }

    fn signature_help(
        &mut self,
        params: SignatureHelpParams,
    ) -> BoxFuture<'static, Result<Option<SignatureHelp>>> {
        let backend = self.clone();
        Box::pin(async move { backend._signature_help(params).await })
    }

    fn document_symbol(
        &mut self,
        params: DocumentSymbolParams,
    ) -> BoxFuture<'static, Result<Option<DocumentSymbolResponse>>> {
        let backend = self.clone();
        Box::pin(async move { backend._document_symbol(params).await })
    }

    fn semantic_tokens_full(
        &mut self,
        params: SemanticTokensParams,
    ) -> BoxFuture<'static, Result<Option<SemanticTokensResult>>> {
        let backend = self.clone();
        Box::pin(async move { backend._semantic_tokens_full(params).await })
    }

    fn references(
        &mut self,
        params: ReferenceParams,
    ) -> BoxFuture<'static, Result<Option<Vec<Location>>>> {
        let backend = self.clone();
        Box::pin(async move { backend._references(params).await })
    }

    fn prepare_rename(
        &mut self,
        params: TextDocumentPositionParams,
    ) -> BoxFuture<'static, Result<Option<PrepareRenameResponse>>> {
        let backend = self.clone();
        Box::pin(async move { backend._prepare_rename(params).await })
    }

    fn rename(
        &mut self,
        params: RenameParams,
    ) -> BoxFuture<'static, Result<Option<WorkspaceEdit>>> {
        let backend = self.clone();
        Box::pin(async move { backend._rename(params).await })
    }

    fn completion(
        &mut self,
        params: CompletionParams,
    ) -> BoxFuture<'static, Result<Option<CompletionResponse>>> {
        let backend = self.clone();
        Box::pin(async move { backend._completion(params).await })
    }

    fn formatting(
        &mut self,
        params: DocumentFormattingParams,
    ) -> BoxFuture<'static, Result<Option<Vec<TextEdit>>>> {
        let backend = self.clone();
        Box::pin(async move { backend._formatting(params).await })
    }

    fn code_action(
        &mut self,
        params: CodeActionParams,
    ) -> BoxFuture<'static, Result<Option<CodeActionResponse>>> {
        let backend = self.clone();
        Box::pin(async move { backend._code_action(params).await })
    }
}

impl Backend {
    async fn _initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Capture base scripts path from initializationOptions if provided.
        // workspace/configuration is pulled after initialized(), but this ensures
        // we have a value even before that round-trip completes.
        if let Some(opts) = &params.initialization_options {
            if let Some(p) = opts.get("gameDirectory").and_then(|v| v.as_str()) {
                if !p.is_empty() {
                    *self.base_scripts_path.lock().await = Some(PathBuf::from(p));
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
                *self.additional_script_dirs.lock().await = dirs;
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
                *self.legacy_script_dirs.lock().await = dirs;
            }
            let mut cfg = (**self.config.load()).clone();
            if let Some(b) = opts
                .get("autoLoadModSharedImports")
                .and_then(|v| v.as_bool())
            {
                cfg.auto_load_mod_shared_imports = b;
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
        let game_dir = self.base_scripts_path.lock().await.clone();
        info!(roots = ?roots, game_dir = ?game_dir, "LSP initialize");
        *self.workspace_roots.lock().await = roots;

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

    async fn _code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let roots = self.workspace_roots.lock().await.clone();
        let actions = base_script_conflict_code_actions(&params.context.diagnostics, &roots);
        Ok((!actions.is_empty()).then_some(actions))
    }

    async fn _initialized(&self, _: InitializedParams) {
        tracing::trace!("initialized: fetching config and indexing");
        self.fetch_config().await;
        self.index_workspace().await;
        self.register_file_watchers().await;
        self.index_base_scripts().await;
        self.initial_index_done.store(true, Ordering::Release);
        self.publish_open_diagnostics().await;
    }

    async fn _did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        // The client drops a file's status on close; force a fresh push.
        self.sent_legacy_status.lock().await.remove(&uri);
        self.sent_file_scope_status.lock().await.remove(&uri);
        self.update_open_document(uri, params.text_document.text)
            .await;
        self.publish_legacy_script_status().await;
        self.publish_file_scope_status().await;
    }

    #[tracing::instrument(skip_all, fields(uri = %params.text_document.uri), level = "debug")]
    async fn _did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        let prior = self
            .documents
            .lock()
            .await
            .get(&uri)
            .map(|d| (d.source.clone(), d.line_index.clone()));

        let Some((mut source, mut line_index)) = prior else {
            error!(uri = %uri, "did_change before did_open");
            return;
        };

        for change in params.content_changes {
            let range = change
                .range
                .map(|r| source_range(source_position(r.start), source_position(r.end)));
            match apply_content_change(&source, &line_index, range, &change.text) {
                Some(next) => {
                    line_index = LineIndex::new(&next);
                    source = next;
                }
                None => {
                    error!(uri = %uri, "out-of-range incremental change; dropping batch");
                    return;
                }
            }
        }

        self.update_open_document(uri, source).await;
    }

    async fn _did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        let scope = self.file_scope_of(&uri).await;
        self.documents.lock().await.remove(&uri);
        if scope.is_loose() {
            // A loose file is a transient compilation member: closing it drops it from the index entirely.
            let invalidated = {
                let mut index = self.loose_index.lock().await;
                crate::indexing::remove_document_all_spellings(&mut index, &uri)
            };
            self.evict_cache_entries(&invalidated).await;
        } else {
            self.reindex_closed_file(&uri).await;
        }
        self.publish_open_diagnostics().await;
        self.publish_file_scope_status().await;
        self.sent_file_scope_status.lock().await.remove(&uri);
    }

    async fn _did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.apply_watched_file_events(params.changes).await;
    }

    async fn _did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let removed: Vec<PathBuf> = params
            .event
            .removed
            .iter()
            .filter_map(|folder| folder.uri.to_file_path().ok())
            .collect();
        let added: Vec<PathBuf> = params
            .event
            .added
            .iter()
            .filter_map(|folder| folder.uri.to_file_path().ok())
            .collect();

        {
            let mut roots = self.workspace_roots.lock().await;
            roots.retain(|root| !removed.iter().any(|dir| root.starts_with(dir)));
            for path in &added {
                if !roots.contains(path) {
                    roots.push(path.clone());
                }
            }
        }

        // index_workspace only adds files; a removed folder's scripts must be dropped here.
        if !removed.is_empty() {
            let invalidated = {
                let mut index = self.workspace_index.lock().await;
                let mut docs = self.workspace_documents.lock().await;
                let stale: Vec<String> = docs
                    .keys()
                    .filter(|uri| uri_within_any(uri, &removed))
                    .cloned()
                    .collect();
                let mut invalidated: HashSet<String> = HashSet::new();
                for uri in stale {
                    invalidated.extend(index.remove_document(&uri));
                    docs.remove(&uri);
                }
                invalidated
            };
            self.evict_cache_entries(&invalidated).await;
        }

        self.index_workspace().await;
        self.reindex_open_documents().await;
        self.publish_open_diagnostics().await;
        self.publish_file_scope_status().await;
    }

    async fn _did_change_configuration(&self, _: DidChangeConfigurationParams) {
        let initial_done = self.initial_index_done.load(Ordering::Acquire);
        let change = self.fetch_config().await;
        if !initial_done {
            tracing::trace!("did_change_configuration: startup echo, skipping re-index");
            return;
        }
        if change.needs_reindex {
            tracing::trace!("did_change_configuration: index-relevant config changed, re-indexing");
            self.index_workspace().await;
            self.index_base_scripts().await;
            self.reindex_open_documents().await;
            self.publish_open_diagnostics().await;
        } else {
            tracing::trace!(
                "did_change_configuration: no index-relevant config change, skipping re-index"
            );
        }
        if change.diagnostics_changed {
            tracing::trace!("did_change_configuration: diagnostics setting changed, applying");
            self.reconcile_published_diagnostics().await;
        }
        self.publish_file_scope_status().await;
    }

    async fn _definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri).await;
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
    }

    async fn _hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri).await;
        let db = handles.db();
        let Some(definition) =
            resolve_definition(uri.as_str(), document, &db, source_position(position))
        else {
            return Ok(None);
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_markdown(&definition),
            }),
            range: Some(lsp_range(definition.symbol.selection_range)),
        }))
    }

    async fn _signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri).await;
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
    }

    async fn _document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        Ok(Some(DocumentSymbolResponse::Nested(document_symbols(
            &document.symbols,
            None,
            params.text_document.uri.as_str(),
        ))))
    }

    async fn _semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri).await;
        let db = handles.db();
        let data = collect_semantic_tokens(uri.as_str(), document, &db);
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
    }

    async fn _references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri).await;
        let db = handles.db();

        let ws_kb = handles.workspace().doc_idents_bytes() / 1024;
        let base_kb = handles.base().doc_idents_bytes() / 1024;
        info!(
            ws_kb,
            base_kb,
            total_kb = ws_kb + base_kb,
            "ident index memory"
        );

        let Some(definition) =
            resolve_definition(uri.as_str(), document, &db, source_position(position))
        else {
            return Ok(Some(Vec::new()));
        };

        let workspace_docs = self.workspace_documents.lock().await;
        let base_docs = self.base_scripts_documents.lock().await;
        let loose_uris = self.loose_open_uris(&documents).await;
        let target_is_loose = loose_uris.contains(&uri);

        let merged = merge_documents(
            &base_docs,
            &workspace_docs,
            &documents,
            &loose_uris,
            target_is_loose,
        );

        let definition_document = merged.get(&definition.uri).copied().unwrap_or(document);

        let search_docs: Vec<(&str, &ParsedDocument)> = merged
            .iter()
            .map(|(uri, doc)| (uri.as_str(), *doc))
            .collect();

        let refs = find_references(
            &definition,
            definition_document,
            &search_docs,
            &db,
            include_declaration,
        );

        let locations: Vec<Location> = refs
            .into_iter()
            .filter_map(|(ref_uri, range)| {
                Url::parse(&ref_uri).ok().map(|url| Location {
                    uri: url,
                    range: lsp_range(range),
                })
            })
            .collect();

        Ok(Some(locations))
    }

    async fn _prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let Some(definition) = self
            .resolve_at(&params.text_document.uri, params.position)
            .await
        else {
            return Ok(None);
        };

        let base_docs = self.base_scripts_documents.lock().await;
        if base_docs.contains_key(&definition.uri) {
            return Err(ResponseError::new(
                ErrorCode::INVALID_REQUEST,
                "Cannot rename a symbol declared in a base script (read-only)",
            ));
        }
        if builtin_source(&definition.uri).is_some() {
            return Err(ResponseError::new(
                ErrorCode::INVALID_REQUEST,
                "Cannot rename a built-in symbol",
            ));
        }

        Ok(Some(PrepareRenameResponse::DefaultBehavior {
            default_behavior: true,
        }))
    }

    async fn _rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let new_name = params.new_name;

        let Some(definition) = self
            .resolve_at(&uri, params.text_document_position.position)
            .await
        else {
            return Ok(None);
        };

        let documents = self.documents.lock().await;
        let handles = self.db_handles_for(&uri).await;
        let workspace_docs = self.workspace_documents.lock().await;
        let base_docs = self.base_scripts_documents.lock().await;

        if base_docs.contains_key(&definition.uri) {
            return Err(ResponseError::new(
                ErrorCode::INVALID_REQUEST,
                "Cannot rename a symbol declared in a base script (read-only)",
            ));
        }
        if builtin_source(&definition.uri).is_some() {
            return Err(ResponseError::new(
                ErrorCode::INVALID_REQUEST,
                "Cannot rename a built-in symbol",
            ));
        }

        let db = handles.db();

        let loose_uris = self.loose_open_uris(&documents).await;
        let merged = merge_documents(
            &base_docs,
            &workspace_docs,
            &documents,
            &loose_uris,
            loose_uris.contains(&uri),
        );

        let Some(definition_document) = merged.get(&definition.uri).copied() else {
            return Ok(None);
        };

        let search_docs: Vec<(&str, &ParsedDocument)> = merged
            .iter()
            .map(|(uri, doc)| (uri.as_str(), *doc))
            .collect();

        let refs = find_references(&definition, definition_document, &search_docs, &db, true);

        let changes = rename_changes(&refs, &new_name, &base_docs);

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }))
    }

    async fn _completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri).await;
        let db = handles.db();

        let pos = source_position(position);

        let member_items: Vec<CompletionItem> =
            completion_members(uri.as_str(), document, &db, pos)
                .iter()
                .map(|(tier, def)| {
                    let params = db.parameters_of(&def.uri, def.symbol.id);
                    let mut item = completion_item(def, &params);
                    item.sort_text = Some(format!("{}_{}", tier, def.symbol.name));
                    item
                })
                .collect();
        if !member_items.is_empty() {
            return Ok(Some(CompletionResponse::Array(member_items)));
        }

        let annotation_arg = annotation_arg_completions(document, &db, pos);
        if !annotation_arg.is_empty() {
            return Ok(Some(CompletionResponse::Array(
                annotation_arg.iter().map(type_completion_item).collect(),
            )));
        }

        if let Some(at_pos) = annotation_name_completions(document, pos) {
            let replace_range = lsp_range(source_range(at_pos, pos));
            return Ok(Some(CompletionResponse::Array(annotation_name_items(
                replace_range,
            ))));
        }

        match after_wrap_method_completions(document, &db, pos) {
            Some(AfterWrapMethodCompletions::FunctionKeyword) => {
                return Ok(Some(CompletionResponse::Array(vec![keyword_snippet_item(
                    "function", "function",
                )])));
            }
            Some(AfterWrapMethodCompletions::MethodList(methods)) => {
                let items = methods
                    .iter()
                    .map(|def| {
                        let snippet = wrap_method_snippet(def, &db);
                        CompletionItem {
                            label: def.symbol.name.clone(),
                            kind: Some(CompletionItemKind::METHOD),
                            detail: def.symbol.signature.clone(),
                            insert_text: Some(snippet),
                            insert_text_format: Some(InsertTextFormat::SNIPPET),
                            ..CompletionItem::default()
                        }
                    })
                    .collect();
                return Ok(Some(CompletionResponse::Array(items)));
            }
            None => {}
        }

        let extends = extends_completions(document, &db, pos);
        if !extends.is_empty() {
            return Ok(Some(CompletionResponse::Array(
                extends.iter().map(type_completion_item).collect(),
            )));
        }

        let state_owners = state_owner_completions(document, &db, pos);
        if !state_owners.is_empty() {
            return Ok(Some(CompletionResponse::Array(
                state_owners.iter().map(type_completion_item).collect(),
            )));
        }

        let header_kws = class_header_keyword_completions(document, pos);
        if !header_kws.is_empty() {
            return Ok(Some(CompletionResponse::Array(
                header_kws
                    .iter()
                    .map(|kw| keyword_snippet_item(kw, &format!("{kw} ")))
                    .collect(),
            )));
        }

        let user_types = type_completions(document, &db, pos);
        if !user_types.is_empty() {
            let mut items: Vec<CompletionItem> = BUILTIN_TYPE_COMPLETIONS
                .iter()
                .map(|name| builtin_type_item(name))
                .collect();
            items.extend(user_types.iter().map(type_completion_item));
            return Ok(Some(CompletionResponse::Array(items)));
        }

        let class_body_kws = class_body_keyword_completions(document, pos);
        if !class_body_kws.is_empty() {
            return Ok(Some(CompletionResponse::Array(
                class_body_kws
                    .iter()
                    .map(|kw| class_body_kw_item(kw))
                    .collect(),
            )));
        }

        let script_body_kws = script_body_completions(document, pos);
        if !script_body_kws.is_empty() {
            return Ok(Some(CompletionResponse::Array(
                script_body_kws
                    .iter()
                    .map(|kw| script_body_item(kw))
                    .collect(),
            )));
        }

        let stmt = statement_completions(uri.as_str(), document, &db, pos);
        if stmt.has_this
            || stmt.has_super
            || !stmt.locals.is_empty()
            || !stmt.members.is_empty()
            || !stmt.globals.is_empty()
        {
            let mut items: Vec<CompletionItem> = Vec::new();
            if stmt.has_this {
                items.push(this_super_item("this"));
            }
            if stmt.has_super {
                items.push(this_super_item("super"));
            }
            items.push(keyword_snippet_item("var", "var ${1:name} : ${2:Type};"));
            items.push(keyword_snippet_item("if", "if (${1:condition})"));
            items.push(keyword_snippet_item("else", "else"));
            items.push(keyword_snippet_item("return", "return;"));
            items.push(keyword_snippet_item(
                "for",
                "for (${1:init}; ${2:condition}; ${3:increment}) {\n\t$0\n}",
            ));
            items.push(keyword_snippet_item(
                "while",
                "while (${1:condition}) {\n\t$0\n}",
            ));
            items.push(keyword_snippet_item(
                "do",
                "do {\n\t$0\n} while (${1:condition});",
            ));
            items.push(keyword_snippet_item(
                "switch",
                "switch (${1:expr}) {\n\tcase ${2:val}:\n\t\t$0\n\t\tbreak;\n}",
            ));
            if stmt.in_switch {
                items.push(keyword_snippet_item("case", "case ${1:val}: $0"));
                items.push(keyword_snippet_item("default", "default: $0"));
                items.push(keyword_snippet_item("break", "break;"));
            }
            if stmt.in_loop {
                items.push(keyword_snippet_item("break", "break;"));
                items.push(keyword_snippet_item("continue", "continue;"));
            }
            for def in &stmt.locals {
                let params = db.parameters_of(&def.uri, def.symbol.id);
                let mut item = completion_item(def, &params);
                item.sort_text = Some(format!("0_{}", def.symbol.name));
                items.push(item);
            }
            for def in &stmt.members {
                let params = db.parameters_of(&def.uri, def.symbol.id);
                let mut item = completion_item(def, &params);
                item.sort_text = Some(format!("1_{}", def.symbol.name));
                items.push(item);
            }
            for def in &stmt.globals {
                let params = db.parameters_of(&def.uri, def.symbol.id);
                let mut item = completion_item(def, &params);
                item.sort_text = Some(format!("2_{}", def.symbol.name));
                items.push(item);
            }
            return Ok(Some(CompletionResponse::Array(items)));
        }

        if let Some(expr) = expression_completions(uri.as_str(), document, &db, pos) {
            let mut items: Vec<CompletionItem> = Vec::new();
            if expr.has_this {
                items.push(this_super_item("this"));
            }
            if expr.has_super {
                items.push(this_super_item("super"));
            }
            items.push(keyword_snippet_item("true", "true"));
            items.push(keyword_snippet_item("false", "false"));
            for def in &expr.locals {
                let params = db.parameters_of(&def.uri, def.symbol.id);
                let mut item = completion_item(def, &params);
                item.sort_text = Some(format!("0_{}", def.symbol.name));
                items.push(item);
            }
            for def in &expr.members {
                let params = db.parameters_of(&def.uri, def.symbol.id);
                let mut item = completion_item(def, &params);
                item.sort_text = Some(format!("0_{}", def.symbol.name));
                items.push(item);
            }
            for def in &expr.globals {
                let params = db.parameters_of(&def.uri, def.symbol.id);
                let mut item = completion_item(def, &params);
                item.sort_text = Some(format!("2_{}", def.symbol.name));
                items.push(item);
            }
            return Ok(Some(CompletionResponse::Array(items)));
        }

        Ok(None)
    }

    async fn _formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return Ok(None);
        }
        let tab_size = params.options.tab_size;
        let use_tabs = !params.options.insert_spaces;

        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };

        let cfg = self.config.load();
        let line_limit = cfg.formatter_line_limit;
        let compact_colon = cfg.formatter_compact_colon;
        let align_member_colons = cfg.formatter_align_member_colons;

        let formatted = format_document(
            document.tree.root_node(),
            &document.source,
            tab_size,
            use_tabs,
            line_limit,
            compact_colon,
            align_member_colons,
        );

        if formatted == document.source {
            return Ok(Some(Vec::new()));
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
    }
}
