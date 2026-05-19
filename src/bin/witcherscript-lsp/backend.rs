use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::{ClientSocket, ErrorCode, LanguageServer, ResponseError};
use futures::future::BoxFuture;
use lsp_types::notification::PublishDiagnostics;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, InsertTextFormat, Location,
    MarkupContent, MarkupKind, OneOf, PrepareRenameResponse, PublishDiagnosticsParams,
    ReferenceParams, RenameOptions, RenameParams, SemanticToken, SemanticTokenModifier,
    SemanticTokenType, SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, SignatureHelp, SignatureHelpOptions,
    SignatureHelpParams, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Url, WorkDoneProgressOptions, WorkspaceEdit,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};
use witcherscript_language::builtins::builtin_source;
use witcherscript_language::document::{apply_content_change, ParsedDocument};
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

use crate::config::Config;
use crate::convert::{
    annotation_name_items, builtin_type_item, class_body_kw_item, completion_item,
    document_symbols, hover_markdown, keyword_snippet_item, lsp_range, script_body_item,
    signature_help_response, source_position, source_range, this_super_item, type_completion_item,
    workspace_roots, wrap_method_snippet,
};
use crate::logging::{level_from_str, level_to_u8};

type Result<T> = std::result::Result<T, ResponseError>;

pub(crate) enum DocOp {
    Open(DidOpenTextDocumentParams),
    Change(DidChangeTextDocumentParams),
    Close(DidCloseTextDocumentParams),
    WatchedFiles(DidChangeWatchedFilesParams),
}

// Open editor docs shadow workspace docs which shadow base docs — unsaved edits win.
pub(crate) fn merge_documents<'a>(
    base_docs: &'a HashMap<String, ParsedDocument>,
    workspace_docs: &'a HashMap<String, ParsedDocument>,
    open_documents: &'a HashMap<Url, ParsedDocument>,
) -> HashMap<String, &'a ParsedDocument> {
    let mut merged: HashMap<String, &ParsedDocument> = HashMap::new();
    for (uri, doc) in base_docs.iter() {
        merged.insert(uri.clone(), doc);
    }
    for (uri, doc) in workspace_docs.iter() {
        merged.insert(uri.clone(), doc);
    }
    for (url, doc) in open_documents.iter() {
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
    pub(crate) base_scripts_index: Arc<Mutex<WorkspaceIndex>>,
    pub(crate) base_scripts_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>,
    pub(crate) builtins_index: Arc<WorkspaceIndex>,
    pub(crate) script_env: Arc<Mutex<ScriptEnvironment>>,
    pub(crate) cst_diag_cache: Arc<Mutex<HashMap<Url, crate::cst_cache::CstCacheEntry>>>,
    pub(crate) initial_index_done: Arc<AtomicBool>,
    pub(crate) doc_ops_tx: mpsc::UnboundedSender<DocOp>,
}

impl Backend {
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
        }
    }

    fn submit_doc_op(&self, op: DocOp) {
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
        tokio::spawn(async move { backend._initialized(params).await });
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
        tokio::spawn(async move { backend._did_change_configuration(params).await });
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
            let mut cfg = (**self.config.load()).clone();
            if let Some(b) = opts
                .get("autoLoadModSharedImports")
                .and_then(|v| v.as_bool())
            {
                cfg.auto_load_mod_shared_imports = b;
            }
            if let Some(diag) = opts.get("diagnostics") {
                if let Some(b) = diag.get("enable").and_then(|v| v.as_bool()) {
                    cfg.diagnostics_enabled = b;
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
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: None,
                    }),
                    ..WorkspaceServerCapabilities::default()
                }),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
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
        if builtin_source(params.text_document.uri.as_str()).is_some() {
            return;
        }
        self.update_open_document(params.text_document.uri, params.text_document.text)
            .await;
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
        let _ = self
            .client
            .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                uri: params.text_document.uri,
                diagnostics: Vec::new(),
                version: None,
            });
    }

    async fn _did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.apply_watched_file_events(params.changes).await;
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
        } else {
            tracing::trace!(
                "did_change_configuration: no index-relevant config change, skipping re-index"
            );
        }
        if change.diagnostics_toggled {
            tracing::trace!("did_change_configuration: diagnostics toggle changed, applying");
            self.apply_diagnostics_toggle().await;
        }
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
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base)
            .with_script_env(&script_env)
            .with_builtins(&self.builtins_index);
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
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base)
            .with_script_env(&script_env)
            .with_builtins(&self.builtins_index);
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
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base)
            .with_script_env(&script_env)
            .with_builtins(&self.builtins_index);
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
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base)
            .with_script_env(&script_env)
            .with_builtins(&self.builtins_index);
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
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base)
            .with_script_env(&script_env)
            .with_builtins(&self.builtins_index);

        let ws_kb = workspace.doc_idents_bytes() / 1024;
        let base_kb = base.doc_idents_bytes() / 1024;
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

        let merged = merge_documents(&base_docs, &workspace_docs, &documents);

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
        let workspace = self.workspace_index.lock().await;
        let base_index = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
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

        let db = SymbolDb::new(&workspace, &base_index)
            .with_script_env(&script_env)
            .with_builtins(&self.builtins_index);

        let merged = merge_documents(&base_docs, &workspace_docs, &documents);

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
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base)
            .with_script_env(&script_env)
            .with_builtins(&self.builtins_index);

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
