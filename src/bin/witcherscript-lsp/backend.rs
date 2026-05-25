use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::{ClientSocket, ErrorCode, LanguageServer, ResponseError};
use futures::future::BoxFuture;
use lsp_types::{
    CodeActionParams, CodeActionResponse, CompletionParams, CompletionResponse, Diagnostic,
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, InitializeParams, InitializeResult,
    InitializedParams, Location, PrepareRenameResponse, ReferenceParams, RenameParams,
    SemanticTokensParams, SemanticTokensResult, SignatureHelp, SignatureHelpParams,
    TextDocumentPositionParams, TextEdit, Url, WorkspaceEdit,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex, MutexGuard};
use tracing::{error, trace};
use witcherscript_language::builtins::{builtin_source, load_builtins_index};
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::{SymbolDb, WorkspaceIndex};
use witcherscript_language::script_env::ScriptEnvironment;

use crate::config::Config;
use crate::file_scope::{classify_file_scope, FileScope};
use crate::file_scope_status::FileScopeStatusParams;
use crate::legacy_status::LegacyScriptStatusParams;

type Result<T> = std::result::Result<T, ResponseError>;

pub(crate) enum DocOp {
    Open(DidOpenTextDocumentParams),
    Change(DidChangeTextDocumentParams),
    Close(DidCloseTextDocumentParams),
    WatchedFiles(DidChangeWatchedFilesParams),
    WorkspaceFolders(DidChangeWorkspaceFoldersParams),
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
    // Canonical URIs the workspace walker yielded; "under a root but missing" means gitignored.
    pub(crate) workspace_known_files: Arc<Mutex<HashSet<String>>>,
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
            workspace_known_files: Arc::new(Mutex::new(HashSet::new())),
            loose_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            builtins_index: Arc::new(load_builtins_index()),
            script_env: Arc::new(Mutex::new(ScriptEnvironment::default())),
            cst_diag_cache: Arc::new(Mutex::new(HashMap::new())),
            initial_index_done: Arc::new(AtomicBool::new(false)),
            doc_ops_tx,
        }
    }

    pub(crate) async fn exclude_filter(&self) -> witcherscript_language::files::ExcludeFilter {
        let roots = self.workspace_roots.lock().await.clone();
        let globs = self.files_exclude.lock().await.clone();
        witcherscript_language::files::ExcludeFilter::new(&roots, &globs)
    }

    pub(crate) async fn is_uri_excluded(&self, uri: &Url) -> bool {
        let Ok(path) = uri.to_file_path() else {
            return false;
        };
        let roots = self.workspace_roots.lock().await;
        if !roots.iter().any(|r| path.starts_with(r)) {
            return false;
        }
        drop(roots);
        // The set isn't authoritative until the startup walk has populated it.
        if !self.initial_index_done.load(Ordering::Acquire) {
            return false;
        }
        let Some(canonical) = canonical_uri(uri) else {
            return false;
        };
        !self.workspace_known_files.lock().await.contains(&canonical)
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
