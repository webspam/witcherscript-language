use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use arc_swap::ArcSwap;
use async_lsp::{ClientSocket, ErrorCode, LanguageServer, ResponseError};
use futures::future::BoxFuture;
use lsp_types::request::Request as LspRequest;
use lsp_types::{
    CodeActionParams, CodeActionResponse, CodeLens, CodeLensParams, CompletionParams,
    CompletionResponse, DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentDiagnosticParams, DocumentDiagnosticReportResult, DocumentFormattingParams,
    DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverParams, InitializeParams, InitializeResult, InitializedParams, InlayHint,
    InlayHintParams, Location, PrepareRenameResponse, ReferenceParams, RenameParams,
    SemanticTokensParams, SemanticTokensResult, SignatureHelp, SignatureHelpParams,
    TextDocumentPositionParams, TextEdit, Url, WorkspaceDiagnosticParams,
    WorkspaceDiagnosticReportResult, WorkspaceEdit,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tracing::{debug, trace};

use witcherscript_language::builtins::{builtin_source, load_builtins_index};
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::{
    FilteredBaseCatalogs, ObservedKey, SubscriptionRegistry, SymbolDb, WorkspaceIndex,
};
use witcherscript_language::script_env::ScriptEnvironment;

use crate::compilation::{Compilation, CompilationBuilder};
use crate::completion_cache::MergedCompletionCache;
use crate::config::Config;
use crate::edit_queue::PendingEdit;
use crate::file_scope::{FileScope, classify_file_scope};
use crate::file_scope_status::FileScopeStatusParams;
use crate::legacy_status::LegacyScriptStatusParams;

type Result<T> = std::result::Result<T, ResponseError>;

const BASE_SCRIPTS_SUBDIR: &str = r"content\content0\scripts";

// The diagnosed set excludes read-only base scripts, so it cannot reuse merge_documents.
pub(crate) fn diagnostics_document_set<'a>(
    workspace_docs: &'a HashMap<String, Arc<ParsedDocument>>,
    open_documents: &'a HashMap<Url, Arc<ParsedDocument>>,
    whole_workspace: bool,
) -> HashMap<String, &'a ParsedDocument> {
    let mut merged: HashMap<String, &ParsedDocument> = HashMap::new();
    if whole_workspace {
        for (uri, doc) in workspace_docs {
            merged.insert(uri.clone(), doc.as_ref());
        }
    }
    for (url, doc) in open_documents {
        merged.insert(canonical_uri(url), doc.as_ref());
    }
    merged
}

// Single-URI form of `diagnostics_document_set`; the index fallback is what lets a restored-but-unopened tab get diagnostics.
pub(crate) fn diagnostics_document_for(
    workspace_docs: &HashMap<String, Arc<ParsedDocument>>,
    open_documents: &HashMap<Url, Arc<ParsedDocument>>,
    uri: &Url,
    whole_workspace: bool,
) -> Option<Arc<ParsedDocument>> {
    if let Some(doc) = open_documents.get(uri) {
        return Some(doc.clone());
    }
    if whole_workspace {
        return workspace_docs.get(&canonical_uri(uri)).cloned();
    }
    None
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
    // Single atomically-published snapshot containing every field a reader handler needs.
    // Readers do one ArcSwap::load_full() and read lock-free for the handler duration.
    pub(crate) compilation: Arc<ArcSwap<Compilation>>,
    // Writers serialize through this; the writer never blocks readers because mutation happens
    // on a shadow Compilation that is then atomically swapped into `compilation`.
    pub(crate) writer_lock: Arc<Mutex<()>>,
    pub(crate) workspace_roots: Arc<ArcSwap<Vec<PathBuf>>>,
    // Mutex not ArcSwap: per-key insert/remove delta from manifest watch events.
    pub(crate) manifest_legacy_dirs: Arc<Mutex<HashMap<String, PathBuf>>>,
    pub(crate) legacy_replacements: Arc<ArcSwap<HashMap<String, String>>>,
    // Mutex not ArcSwap: check-then-insert dedup must stay atomic so each status sends once.
    pub(crate) sent_legacy_status: Arc<Mutex<HashMap<Url, LegacyScriptStatusParams>>>,
    pub(crate) sent_file_scope_status: Arc<Mutex<HashMap<Url, FileScopeStatusParams>>>,
    // Canonical URIs the workspace walker yielded; "under a root but missing" means gitignored.
    pub(crate) workspace_known_files: Arc<Mutex<HashSet<String>>>,
    pub(crate) builtins_index: Arc<WorkspaceIndex>,
    // Subscribers are bookkeeping for CST-cache invalidation; lifted out of WorkspaceIndex so the
    // index itself is read-only from the publisher's point of view.
    pub(crate) workspace_subscriptions: Arc<Mutex<SubscriptionRegistry>>,
    pub(crate) loose_subscriptions: Arc<Mutex<SubscriptionRegistry>>,
    // Mutex not ArcSwap: caches read then commit computed misses under one held lock.
    pub(crate) cst_diag_cache: Arc<Mutex<HashMap<String, crate::cst_cache::CstCacheEntry>>>,
    // Whole-workspace diagnostics keyed by surface fingerprint; a key mismatch is the sole invalidation.
    pub(crate) diag_bundle_cache: Arc<Mutex<Option<crate::diagnostics_publish::CachedBundle>>>,
    pub(crate) merged_completion_cache_workspace: Arc<Mutex<Option<MergedCompletionCache>>>,
    pub(crate) merged_completion_cache_loose: Arc<Mutex<Option<MergedCompletionCache>>>,
    pub(crate) initial_index_done: Arc<AtomicBool>,
    pub(crate) legacy_db_generation: Arc<AtomicU64>,
    pub(crate) state_version: Arc<AtomicU64>,
    pub(crate) client_supports_pull_diagnostics: Arc<AtomicBool>,
    pub(crate) client_supports_code_lens_refresh: Arc<AtomicBool>,
    pub(crate) client_supports_semantic_tokens_refresh: Arc<AtomicBool>,
    pub(crate) client_supports_inlay_hint_refresh: Arc<AtomicBool>,
    // Mutex not ArcSwap: edit queue with version-checked insert/remove.
    pub(crate) pending_edits: Arc<Mutex<HashMap<Url, PendingEdit>>>,
    pub(crate) edit_notify: Arc<tokio::sync::Notify>,
    pub(crate) edit_writer_spawned: Arc<AtomicBool>,
    // Raised by every view-relevant swap; the refresher coalesces these (single-permit Notify folds a burst).
    pub(crate) views_dirty: Arc<tokio::sync::Notify>,
    pub(crate) view_refresher_spawned: Arc<AtomicBool>,
    // Wakes handlers blocked in `await_initial_index`; paired with `initial_index_done`.
    pub(crate) index_ready_notify: Arc<tokio::sync::Notify>,
}

#[cfg(test)]
impl Backend {
    pub(crate) fn update_config(&self, f: impl FnOnce(&mut Config)) {
        let mut cfg = (**self.config.load()).clone();
        f(&mut cfg);
        self.config.store(Arc::new(cfg));
    }

    pub(crate) fn set_workspace_roots(&self, roots: Vec<PathBuf>) {
        self.workspace_roots.store(Arc::new(roots));
    }
}

pub(super) fn build_symbol_db<'a>(
    workspace: &'a WorkspaceIndex,
    base: &'a WorkspaceIndex,
    script_env: &'a ScriptEnvironment,
    builtins: &'a WorkspaceIndex,
    suppressed: &'a HashSet<String>,
    filtered: Option<&'a FilteredBaseCatalogs>,
) -> SymbolDb<'a> {
    let mut db = SymbolDb::new(workspace, base)
        .with_suppressed_base_uris(suppressed)
        .with_script_env(script_env)
        .with_builtins(builtins);
    if let Some(catalogs) = filtered {
        db = db.with_prefiltered_base(catalogs);
    }
    db
}

// Owned Arcs cloned from a Compilation snapshot. No locks held; the handler keeps the snapshot
// alive as long as it holds this struct, so a concurrent publish cannot tear the view.
pub(crate) struct DbHandles {
    workspace: Arc<WorkspaceIndex>,
    base: Arc<WorkspaceIndex>,
    script_env: Arc<ScriptEnvironment>,
    builtins: Arc<WorkspaceIndex>,
    suppressed_base_uris: Arc<HashSet<String>>,
    filtered_base_catalogs: Option<Arc<FilteredBaseCatalogs>>,
}

impl DbHandles {
    pub(crate) fn db(&self) -> SymbolDb<'_> {
        build_symbol_db(
            &self.workspace,
            &self.base,
            &self.script_env,
            &self.builtins,
            &self.suppressed_base_uris,
            self.filtered_base_catalogs.as_deref(),
        )
    }

    pub(crate) fn workspace(&self) -> &WorkspaceIndex {
        &self.workspace
    }

    pub(crate) fn base(&self) -> &WorkspaceIndex {
        &self.base
    }

    pub(crate) fn script_env(&self) -> &ScriptEnvironment {
        &self.script_env
    }
}

impl Backend {
    pub(crate) fn new(client: ClientSocket, config: Arc<ArcSwap<Config>>) -> Backend {
        Backend {
            client,
            config,
            compilation: Arc::new(ArcSwap::from_pointee(Compilation::default())),
            writer_lock: Arc::new(Mutex::new(())),
            workspace_roots: Arc::new(ArcSwap::from_pointee(Vec::new())),
            manifest_legacy_dirs: Arc::new(Mutex::new(HashMap::new())),
            legacy_replacements: Arc::new(ArcSwap::from_pointee(HashMap::new())),
            sent_legacy_status: Arc::new(Mutex::new(HashMap::new())),
            sent_file_scope_status: Arc::new(Mutex::new(HashMap::new())),
            workspace_known_files: Arc::new(Mutex::new(HashSet::new())),
            builtins_index: Arc::new(load_builtins_index()),
            workspace_subscriptions: Arc::new(Mutex::new(SubscriptionRegistry::default())),
            loose_subscriptions: Arc::new(Mutex::new(SubscriptionRegistry::default())),
            cst_diag_cache: Arc::new(Mutex::new(HashMap::new())),
            diag_bundle_cache: Arc::new(Mutex::new(None)),
            merged_completion_cache_workspace: Arc::new(Mutex::new(None)),
            merged_completion_cache_loose: Arc::new(Mutex::new(None)),
            initial_index_done: Arc::new(AtomicBool::new(false)),
            legacy_db_generation: Arc::new(AtomicU64::new(0)),
            state_version: Arc::new(AtomicU64::new(0)),
            client_supports_pull_diagnostics: Arc::new(AtomicBool::new(false)),
            client_supports_code_lens_refresh: Arc::new(AtomicBool::new(false)),
            client_supports_semantic_tokens_refresh: Arc::new(AtomicBool::new(false)),
            client_supports_inlay_hint_refresh: Arc::new(AtomicBool::new(false)),
            pending_edits: Arc::new(Mutex::new(HashMap::new())),
            edit_notify: Arc::new(tokio::sync::Notify::new()),
            edit_writer_spawned: Arc::new(AtomicBool::new(false)),
            views_dirty: Arc::new(tokio::sync::Notify::new()),
            view_refresher_spawned: Arc::new(AtomicBool::new(false)),
            index_ready_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub(crate) fn snapshot(&self) -> Arc<Compilation> {
        self.compilation.load_full()
    }

    // Resolved base scripts dir: the override if set, else the game-dir + scripts subpath.
    pub(crate) fn base_scripts_dir(&self) -> Option<PathBuf> {
        let cfg = self.config.load();
        if let Some(override_dir) = cfg.base_scripts_override.clone() {
            return Some(override_dir);
        }
        cfg.game_directory
            .as_ref()
            .map(|gd| gd.join(BASE_SCRIPTS_SUBDIR))
    }

    // Single-writer publish: build the next Compilation on a shadow and atomically swap.
    // Holding `writer_lock` serializes writers without blocking readers; the swap itself is
    // one atomic pointer write.
    pub(crate) fn publish_compilation<F>(&self, edit: F)
    where
        F: FnOnce(&mut CompilationBuilder),
    {
        let _guard = self.writer_lock.lock();
        let current = self.compilation.load_full();
        let mut builder = CompilationBuilder::new(current);
        edit(&mut builder);
        let changed = builder.changes_views();
        self.compilation.store(Arc::new(builder.finish()));
        if changed {
            self.state_version.fetch_add(1, Ordering::AcqRel);
            self.views_dirty.notify_one();
        }
    }

    pub(crate) fn invalidated_workspace(&self, keys: &[ObservedKey]) -> HashSet<String> {
        self.workspace_subscriptions.lock().subscribers_of(keys)
    }

    pub(crate) fn invalidated_loose(&self, keys: &[ObservedKey]) -> HashSet<String> {
        self.loose_subscriptions.lock().subscribers_of(keys)
    }

    pub(crate) fn db_fingerprint(
        &self,
        base: &WorkspaceIndex,
        env: &ScriptEnvironment,
    ) -> crate::cst_cache::DbFingerprint {
        crate::cst_cache::DbFingerprint {
            base_surface: base.surface_hash(),
            env: env.version(),
            legacy_db_generation: self.legacy_db_generation.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn exclude_filter(&self) -> witcherscript_language::files::ExcludeFilter {
        let roots = self.workspace_roots.load_full();
        let globs = self.config.load().files_exclude.clone();
        witcherscript_language::files::ExcludeFilter::new(&roots, &globs)
    }

    pub(crate) fn is_uri_excluded(&self, uri: &Url) -> bool {
        let Ok(path) = uri.to_file_path() else {
            return false;
        };
        let roots = self.workspace_roots.load();
        if !roots.iter().any(|r| path.starts_with(r)) {
            return false;
        }
        drop(roots);
        // The set isn't authoritative until the startup walk has populated it.
        if !self.initial_index_done.load(Ordering::Acquire) {
            return false;
        }
        let canonical = canonical_uri(uri);
        if self.workspace_known_files.lock().contains(&canonical) {
            return false;
        }
        // Absent from the startup walk means gitignored or created since; the walk can't tell those apart, so ask the ignore rules directly.
        if self.exclude_filter().matches(&path) {
            return true;
        }
        self.workspace_known_files.lock().insert(canonical);
        false
    }

    pub(crate) fn file_scope_of(&self, uri: &Url) -> FileScope {
        let roots = self.workspace_roots.load_full();
        let legacy_dirs = self.effective_legacy_dirs();
        let base_scripts_dir = self.base_scripts_dir();
        let additional = self.config.load().additional_script_dirs.clone();
        let replacements = self.legacy_replacements.load();
        classify_file_scope(
            uri,
            &roots,
            &legacy_dirs,
            &replacements,
            base_scripts_dir.as_deref(),
            &additional,
        )
    }

    // Holds even for an override inside a workspace root, which `file_scope_of` reports as `InProject`, not `LegacyOverride`.
    pub(crate) fn replaces_base_script(&self, uri: &Url) -> bool {
        self.legacy_replacements
            .load()
            .contains_key(&canonical_uri(uri))
    }

    pub(crate) fn loose_open_uris(
        &self,
        documents: &HashMap<Url, Arc<ParsedDocument>>,
    ) -> HashSet<Url> {
        let roots = self.workspace_roots.load_full();
        let legacy_dirs = self.effective_legacy_dirs();
        let base_scripts_dir = self.base_scripts_dir();
        let additional = self.config.load().additional_script_dirs.clone();
        let replacements = self.legacy_replacements.load();
        documents
            .keys()
            .filter(|uri| {
                classify_file_scope(
                    uri,
                    &roots,
                    &legacy_dirs,
                    &replacements,
                    base_scripts_dir.as_deref(),
                    &additional,
                )
                .is_loose()
            })
            .cloned()
            .collect()
    }

    // A loose file resolves against loose_index in the workspace slot, isolating
    // it from the real project's symbols.
    pub(crate) fn db_handles_for_with_snapshot(
        &self,
        uri: &Url,
        snap: &Arc<Compilation>,
    ) -> DbHandles {
        let workspace = if self.file_scope_of(uri).is_loose() {
            snap.loose_index.clone()
        } else {
            snap.workspace_index.clone()
        };
        DbHandles {
            workspace,
            base: snap.base_scripts_index.clone(),
            script_env: snap.script_env.clone(),
            builtins: self.builtins_index.clone(),
            suppressed_base_uris: snap.suppressed_base_uris.clone(),
            filtered_base_catalogs: snap.filtered_base_catalogs.clone(),
        }
    }

    pub(crate) fn rebuild_filtered_base_catalogs(&self) {
        let started_at = std::time::Instant::now();
        debug!(op = "rebuild_filtered_base_catalogs", "start");
        self.publish_compilation(|builder| {
            let suppressed = builder.base.suppressed_base_uris.clone();
            let base_index = builder.base.base_scripts_index.clone();
            let next = if suppressed.is_empty() {
                None
            } else {
                Some(FilteredBaseCatalogs::build(&base_index, &suppressed))
            };
            builder.set_filtered_base_catalogs(next);
        });
        *self.merged_completion_cache_workspace.lock() = None;
        *self.merged_completion_cache_loose.lock() = None;
        self.legacy_db_generation.fetch_add(1, Ordering::Relaxed);
        debug!(
            op = "rebuild_filtered_base_catalogs",
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    pub(crate) fn merged_completion_cache(
        &self,
        uri: &Url,
        handles: &DbHandles,
    ) -> MergedCompletionCache {
        let started_at = std::time::Instant::now();
        debug!(
            op = "merged_completion_cache",
            uri = %uri,
            "start",
        );
        let workspace_surface = handles.workspace().surface_hash();
        let base_surface = handles.base().surface_hash();
        let script_env_version = handles.script_env().version();
        let slot = if self.file_scope_of(uri).is_loose() {
            &self.merged_completion_cache_loose
        } else {
            &self.merged_completion_cache_workspace
        };
        let mut slot = slot.lock();
        if let Some(cached) = slot.as_ref()
            && cached.workspace_surface == workspace_surface
            && cached.base_surface == base_surface
            && cached.script_env_version == script_env_version
        {
            let cached = cached.clone();
            debug!(
                op = "merged_completion_cache",
                uri = %uri,
                cache_hit = true,
                elapsed_us = started_at.elapsed().as_micros(),
                "complete",
            );
            return cached;
        }
        let db = handles.db();
        let fresh = MergedCompletionCache::build(
            handles.workspace(),
            handles.base(),
            &db,
            handles.script_env(),
        );
        *slot = Some(fresh.clone());
        debug!(
            op = "merged_completion_cache",
            uri = %uri,
            cache_hit = false,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        fresh
    }

    // For a change that bypasses publish_compilation: a config/scope toggle mutates `config`, a separate ArcSwap.
    pub(crate) fn mark_state_changed(&self) {
        self.state_version.fetch_add(1, Ordering::AcqRel);
        self.views_dirty.notify_one();
    }

    pub(crate) async fn send_refresh<R: LspRequest<Params = ()>>(client: &ClientSocket)
    where
        R::Result: Send,
    {
        // A refresh is a best-effort nudge to re-pull; if the client declines there is nothing to do, so drop the Err.
        let _ = client.request::<R>(()).await;
    }

    // Offload to a blocking thread so the single async-lsp run task is not frozen by CPU-bound compute.
    pub(crate) async fn spawn_compute<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Backend) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let backend = self.clone();
        match tokio::task::spawn_blocking(move || f(&backend)).await {
            Ok(result) => result,
            Err(join_err) => {
                tracing::error!(error = %join_err, "compute task panicked");
                Err(ResponseError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("compute task failed: {join_err}"),
                ))
            }
        }
    }

    pub(crate) async fn handle_builtin_source(&self, params: Value) -> Result<Value> {
        let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
        let started_at = std::time::Instant::now();
        trace!(op = "builtin_source", uri, "start");
        let result = builtin_source_response(uri);
        trace!(
            op = "builtin_source",
            uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
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
        self._did_open(params);
        ControlFlow::Continue(())
    }

    fn did_change(&mut self, params: DidChangeTextDocumentParams) -> Self::NotifyResult {
        self._did_change(params);
        ControlFlow::Continue(())
    }

    fn did_close(&mut self, params: DidCloseTextDocumentParams) -> Self::NotifyResult {
        self._did_close(params);
        ControlFlow::Continue(())
    }

    fn did_change_watched_files(
        &mut self,
        params: DidChangeWatchedFilesParams,
    ) -> Self::NotifyResult {
        self._did_change_watched_files(params);
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
        Box::pin(async move { backend.spawn_compute(move |b| b._definition(params)).await })
    }

    fn type_definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> BoxFuture<'static, Result<Option<GotoDefinitionResponse>>> {
        let backend = self.clone();
        Box::pin(async move {
            backend
                .spawn_compute(move |b| b._type_definition(params))
                .await
        })
    }

    fn code_lens(
        &mut self,
        params: CodeLensParams,
    ) -> BoxFuture<'static, Result<Option<Vec<CodeLens>>>> {
        let backend = self.clone();
        Box::pin(async move { backend.spawn_compute(move |b| b._code_lens(params)).await })
    }

    fn code_lens_resolve(&mut self, params: CodeLens) -> BoxFuture<'static, Result<CodeLens>> {
        let backend = self.clone();
        Box::pin(async move { backend._code_lens_resolve(params).await })
    }

    fn hover(&mut self, params: HoverParams) -> BoxFuture<'static, Result<Option<Hover>>> {
        let backend = self.clone();
        Box::pin(async move { backend.spawn_compute(move |b| b._hover(params)).await })
    }

    fn signature_help(
        &mut self,
        params: SignatureHelpParams,
    ) -> BoxFuture<'static, Result<Option<SignatureHelp>>> {
        let backend = self.clone();
        Box::pin(async move {
            backend
                .spawn_compute(move |b| b._signature_help(params))
                .await
        })
    }

    fn document_symbol(
        &mut self,
        params: DocumentSymbolParams,
    ) -> BoxFuture<'static, Result<Option<DocumentSymbolResponse>>> {
        let backend = self.clone();
        Box::pin(async move { backend._document_symbol(params) })
    }

    fn semantic_tokens_full(
        &mut self,
        params: SemanticTokensParams,
    ) -> BoxFuture<'static, Result<Option<SemanticTokensResult>>> {
        let backend = self.clone();
        Box::pin(async move {
            backend
                .spawn_compute(move |b| b._semantic_tokens_full(params))
                .await
        })
    }

    fn inlay_hint(
        &mut self,
        params: InlayHintParams,
    ) -> BoxFuture<'static, Result<Option<Vec<InlayHint>>>> {
        let backend = self.clone();
        Box::pin(async move { backend.spawn_compute(move |b| b._inlay_hint(params)).await })
    }

    fn references(
        &mut self,
        params: ReferenceParams,
    ) -> BoxFuture<'static, Result<Option<Vec<Location>>>> {
        let backend = self.clone();
        Box::pin(async move { backend.spawn_compute(move |b| b._references(params)).await })
    }

    fn prepare_rename(
        &mut self,
        params: TextDocumentPositionParams,
    ) -> BoxFuture<'static, Result<Option<PrepareRenameResponse>>> {
        let backend = self.clone();
        Box::pin(async move {
            backend
                .spawn_compute(move |b| b._prepare_rename(params))
                .await
        })
    }

    fn rename(
        &mut self,
        params: RenameParams,
    ) -> BoxFuture<'static, Result<Option<WorkspaceEdit>>> {
        let backend = self.clone();
        Box::pin(async move { backend.spawn_compute(move |b| b._rename(params)).await })
    }

    fn completion(
        &mut self,
        params: CompletionParams,
    ) -> BoxFuture<'static, Result<Option<CompletionResponse>>> {
        let backend = self.clone();
        Box::pin(async move { backend.spawn_compute(move |b| b._completion(params)).await })
    }

    fn formatting(
        &mut self,
        params: DocumentFormattingParams,
    ) -> BoxFuture<'static, Result<Option<Vec<TextEdit>>>> {
        let backend = self.clone();
        Box::pin(async move { backend.spawn_compute(move |b| b._formatting(params)).await })
    }

    fn code_action(
        &mut self,
        params: CodeActionParams,
    ) -> BoxFuture<'static, Result<Option<CodeActionResponse>>> {
        let backend = self.clone();
        Box::pin(async move { backend._code_action(params) })
    }

    fn document_diagnostic(
        &mut self,
        params: DocumentDiagnosticParams,
    ) -> BoxFuture<'static, Result<DocumentDiagnosticReportResult>> {
        let backend = self.clone();
        Box::pin(async move {
            backend.await_initial_index().await;
            backend
                .spawn_compute(move |b| b._document_diagnostic(params))
                .await
        })
    }

    fn workspace_diagnostic(
        &mut self,
        params: WorkspaceDiagnosticParams,
    ) -> BoxFuture<'static, Result<WorkspaceDiagnosticReportResult>> {
        let backend = self.clone();
        Box::pin(async move {
            backend.await_initial_index().await;
            backend
                .spawn_compute(move |b| b._workspace_diagnostic(params))
                .await
        })
    }
}
