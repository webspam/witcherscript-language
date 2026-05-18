use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;

use lsp_types::notification::PublishDiagnostics;
use lsp_types::request::{RegisterCapability, WorkspaceConfiguration};
use lsp_types::{
    ConfigurationItem, ConfigurationParams, Diagnostic, DidChangeWatchedFilesRegistrationOptions,
    FileChangeType, FileEvent, FileSystemWatcher, GlobPattern, Position, PublishDiagnosticsParams,
    Registration, RegistrationParams, Url,
};
use rayon::prelude::*;
use serde_json::Value;
use tracing::{debug, error, info, trace, warn};
use witcherscript_language::diagnostics::{
    collect_duplicate_local_diagnostics, collect_duplicate_symbol_diagnostics,
    collect_shadowing_diagnostics,
};
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::files::{
    collect_witcherscript_files, is_witcherscript_file, read_script_file, ExcludeFilter,
};
use witcherscript_language::resolve::{resolve_definition, Definition, SymbolDb, WorkspaceIndex};
use witcherscript_language::script_env::parse_script_environment;

use crate::backend::Backend;
use crate::convert::{canonical_uri, lsp_diagnostics, lsp_workspace_diagnostic, source_position};
use crate::cst_cache::{cst_diagnostics_with_cache, DbFingerprint};
use crate::logging::{level_from_str, level_to_u8};

fn log_setting_change<T: PartialEq + std::fmt::Display>(setting: &str, prev: T, new: T) {
    if prev != new {
        trace!(setting, prev = %prev, new = %new, "setting changed");
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub(crate) struct ConfigChange {
    pub(crate) needs_reindex: bool,
    pub(crate) diagnostics_toggled: bool,
}

pub(crate) fn build_index_segments(
    game_dir: Option<&Path>,
    extras: &[PathBuf],
    auto_load_mod_shared_imports: bool,
) -> Vec<(&'static str, PathBuf, bool)> {
    let mut segments: Vec<(&'static str, PathBuf, bool)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());

    if let Some(gd) = game_dir {
        let base = gd.join(r"content\content0\scripts");
        if seen.insert(canon(&base)) {
            segments.push(("gameDirectory", base, false));
        }
        if auto_load_mod_shared_imports {
            let msi = gd.join(r"Mods\modSharedImports");
            if msi.is_dir() {
                let key = canon(&msi);
                if !seen.contains(&key) {
                    seen.insert(key);
                    segments.push(("modSharedImports", msi, true));
                }
            }
        }
    }

    for extra in extras {
        if !extra.is_dir() {
            warn!(path = %extra.display(), "additionalScriptDirectories entry is not a directory; skipping");
            continue;
        }
        if seen.insert(canon(extra)) {
            segments.push(("additionalScriptDirectory", extra.clone(), false));
        }
    }

    segments
}

#[tracing::instrument(skip(index, document), fields(uri = %uri), level = "debug")]
pub(crate) fn index_open_document(
    index: &mut WorkspaceIndex,
    uri: &Url,
    document: &ParsedDocument,
) -> HashSet<String> {
    let mut invalidated = HashSet::new();
    if let Some(canonical) = canonical_uri(uri) {
        if canonical != uri.as_str() {
            invalidated.extend(index.remove_document(&canonical));
        }
    }
    invalidated.extend(index.update_document(uri.as_str(), document));
    invalidated
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WatchedEvent {
    Upsert { canonical: String, path: PathBuf },
    Remove { canonical: String },
}

pub(crate) fn classify_watched_event(
    event: &FileEvent,
    open_canonical: &HashSet<String>,
    filter: &ExcludeFilter,
) -> Option<WatchedEvent> {
    let path = event.uri.to_file_path().ok()?;
    if !is_witcherscript_file(&path) {
        return None;
    }
    let canonical = canonical_uri(&event.uri)?;
    if open_canonical.contains(&canonical) {
        return None;
    }
    match event.typ {
        FileChangeType::DELETED => Some(WatchedEvent::Remove { canonical }),
        FileChangeType::CREATED | FileChangeType::CHANGED => {
            if filter.matches(&path) {
                return None;
            }
            Some(WatchedEvent::Upsert { canonical, path })
        }
        _ => None,
    }
}

impl Backend {
    #[tracing::instrument(skip(self, text), fields(uri = %uri, bytes = text.len()), level = "debug")]
    pub(crate) async fn update_open_document(&self, uri: Url, text: String) {
        let parsed = tracing::debug_span!("parse_document").in_scope(|| parse_document(text));
        match parsed {
            Ok(document) => {
                let invalidated = {
                    let mut index = self.workspace_index.lock().await;
                    index_open_document(&mut index, &uri, &document)
                };
                self.evict_cache_entries(&invalidated).await;
                self.documents.lock().await.insert(uri.clone(), document);
                self.publish_open_diagnostics().await;
            }
            Err(err) => {
                error!(uri = %uri, error = %err, "failed to parse document");
            }
        }
    }

    async fn evict_cache_entries(&self, uris: &HashSet<String>) {
        if uris.is_empty() {
            return;
        }
        let mut cache = self.cst_diag_cache.lock().await;
        cache.retain(|url, _| !uris.contains(url.as_str()));
    }

    #[tracing::instrument(skip(self), level = "debug")]
    pub(crate) async fn publish_open_diagnostics(&self) {
        if !self.diagnostics_enabled.load(Ordering::Relaxed) {
            return;
        }

        if !self.initial_index_done.load(Ordering::Acquire) {
            self.publish_syntactic_only().await;
            return;
        }

        let start = Instant::now();

        let documents = self.documents.lock().await;

        let (dup_by_uri, shadow_by_uri, dup_local_by_uri, cst_by_uri, cst_stats) = {
            let mut index = self.workspace_index.lock().await;
            let base = self.base_scripts_index.lock().await;
            let env = self.script_env.lock().await;
            let mut cache = self.cst_diag_cache.lock().await;

            let dup = tracing::debug_span!("dup_symbols")
                .in_scope(|| collect_duplicate_symbol_diagnostics(&index));
            let shadow = tracing::debug_span!("shadowing")
                .in_scope(|| collect_shadowing_diagnostics(&index, &env));
            let dup_local = tracing::debug_span!("dup_locals")
                .in_scope(|| collect_duplicate_local_diagnostics(&index));

            let fingerprint = DbFingerprint {
                base_surface: base.surface_hash(),
                env: env.version(),
            };
            let result = {
                let db = SymbolDb::new(&index, &base)
                    .with_script_env(&env)
                    .with_builtins(&self.builtins_index);
                tracing::debug_span!("cst_diagnostics", open_docs = documents.len()).in_scope(
                    || cst_diagnostics_with_cache(&documents, &db, fingerprint, &mut cache),
                )
            };
            for (uri, observations) in result.new_subscriptions {
                index.register_subscription(&uri, observations);
            }

            (dup, shadow, dup_local, result.by_uri, result.stats)
        };

        let collect_us = start.elapsed().as_micros();

        let to_publish: Vec<(Url, Vec<Diagnostic>)> = {
            let mut published = self.published_diagnostics.lock().await;
            let mut list = Vec::new();
            for (uri, document) in documents.iter() {
                let mut diagnostics = lsp_diagnostics(document);
                if let Some(dups) = dup_by_uri.get(uri.as_str()) {
                    diagnostics.extend(dups.iter().map(lsp_workspace_diagnostic));
                }
                if let Some(shadows) = shadow_by_uri.get(uri.as_str()) {
                    diagnostics.extend(shadows.iter().map(lsp_workspace_diagnostic));
                }
                if let Some(dup_locals) = dup_local_by_uri.get(uri.as_str()) {
                    diagnostics.extend(dup_locals.iter().map(lsp_workspace_diagnostic));
                }
                if let Some(cst) = cst_by_uri.get(uri.as_str()) {
                    diagnostics.extend(cst.iter().map(lsp_workspace_diagnostic));
                }
                if published.get(uri) == Some(&diagnostics) {
                    continue;
                }
                published.insert(uri.clone(), diagnostics.clone());
                list.push((uri.clone(), diagnostics));
            }
            list
        };

        let open_documents = documents.len();
        let flagged_uris = dup_by_uri.len();
        let shadow_uris = shadow_by_uri.len();
        let dup_local_uris = dup_local_by_uri.len();
        let cst_uris = cst_by_uri.len();
        drop(documents);

        let republished = to_publish.len();
        for (uri, diagnostics) in to_publish {
            let _ = self
                .client
                .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                    uri,
                    diagnostics,
                    version: None,
                });
        }

        trace!(
            open_documents,
            flagged_uris,
            shadow_uris,
            dup_local_uris,
            cst_uris,
            cst_cache_hits = cst_stats.hits,
            cst_cache_misses = cst_stats.misses,
            republished,
            collect_us,
            total_us = start.elapsed().as_micros(),
            "recomputed workspace diagnostics for open documents"
        );
    }

    async fn publish_syntactic_only(&self) {
        let to_publish: Vec<(Url, Vec<Diagnostic>)> = {
            let documents = self.documents.lock().await;
            let mut published = self.published_diagnostics.lock().await;
            let mut list = Vec::new();
            for (uri, document) in documents.iter() {
                let diagnostics = lsp_diagnostics(document);
                if published.get(uri) == Some(&diagnostics) {
                    continue;
                }
                published.insert(uri.clone(), diagnostics.clone());
                list.push((uri.clone(), diagnostics));
            }
            list
        };

        for (uri, diagnostics) in to_publish {
            let _ = self
                .client
                .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                    uri,
                    diagnostics,
                    version: None,
                });
        }
    }

    pub(crate) async fn apply_diagnostics_toggle(&self) {
        if self.diagnostics_enabled.load(Ordering::Relaxed) {
            self.publish_open_diagnostics().await;
        } else {
            let uris: Vec<Url> = {
                let mut published = self.published_diagnostics.lock().await;
                let keys: Vec<Url> = published.keys().cloned().collect();
                published.clear();
                keys
            };
            for uri in uris {
                let _ = self
                    .client
                    .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                        uri,
                        diagnostics: Vec::new(),
                        version: None,
                    });
            }
        }
    }

    pub(crate) async fn index_workspace(&self) {
        let roots = self.workspace_roots.lock().await.clone();
        if roots.is_empty() {
            return;
        }
        let exclude_globs = self.files_exclude.lock().await.clone();

        info!(roots = ?roots, "indexing workspace");
        let start = Instant::now();

        let join_result = tokio::task::spawn_blocking(move || {
            let files = match collect_witcherscript_files(&roots, &exclude_globs) {
                Ok(f) => f,
                Err(_) => {
                    warn!("failed to collect workspace files");
                    return None;
                }
            };
            let file_count = files.len();
            let parsed: Vec<(String, ParsedDocument)> = files
                .iter()
                .filter_map(|path| {
                    let source = fs::read_to_string(path)
                        .map_err(|_| warn!(path = %path.display(), "failed to read workspace file"))
                        .ok()?;
                    let document = parse_document(source)
                        .map_err(
                            |_| warn!(path = %path.display(), "failed to parse workspace file"),
                        )
                        .ok()?;
                    let uri = Url::from_file_path(path)
                        .map_err(|_| warn!(path = %path.display(), "failed to convert path to URI"))
                        .ok()?;
                    debug!(uri = %uri, "indexed workspace file");
                    Some((uri.to_string(), document))
                })
                .collect();
            Some((file_count, parsed))
        })
        .await;

        let (file_count, parsed) = match join_result {
            Ok(Some(v)) => v,
            Ok(None) => return,
            Err(err) => {
                error!(error = %err, "workspace indexing task panicked");
                return;
            }
        };

        // Skip files the editor has open; update_open_document keeps them indexed under the client spelling.
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock().await;
            documents.keys().filter_map(canonical_uri).collect()
        };

        let mut indexed = 0;
        {
            let mut index = self.workspace_index.lock().await;
            let mut docs = self.workspace_documents.lock().await;
            for (uri, document) in parsed {
                if open_canonical.contains(&uri) {
                    continue;
                }
                index.update_document(uri.as_str(), &document);
                docs.insert(uri, document);
                indexed += 1;
            }
        }

        info!(
            indexed,
            file_count,
            elapsed_ms = start.elapsed().as_millis(),
            "workspace indexed"
        );

        self.publish_open_diagnostics().await;
    }

    pub(crate) async fn register_file_watchers(&self) {
        let watcher = FileSystemWatcher {
            glob_pattern: GlobPattern::String("**/*.ws".to_string()),
            kind: None,
        };
        let options = DidChangeWatchedFilesRegistrationOptions {
            watchers: vec![watcher],
        };
        let registration = Registration {
            id: "witcherscript-ws-files".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: serde_json::to_value(options).ok(),
        };
        if let Err(err) = self
            .client
            .request::<RegisterCapability>(RegistrationParams {
                registrations: vec![registration],
            })
            .await
        {
            warn!(
                error = %err,
                "failed to register file watcher; workspace index may go stale on external file changes"
            );
        }
    }

    pub(crate) async fn apply_watched_file_events(&self, events: Vec<FileEvent>) {
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock().await;
            documents.keys().filter_map(canonical_uri).collect()
        };
        let roots = self.workspace_roots.lock().await.clone();
        let filter = ExcludeFilter::new(&roots, &self.files_exclude.lock().await.clone());

        let mut updates: Vec<(String, ParsedDocument)> = Vec::new();
        let mut removals: Vec<String> = Vec::new();
        for event in &events {
            let Some(decision) = classify_watched_event(event, &open_canonical, &filter) else {
                continue;
            };
            match decision {
                WatchedEvent::Upsert { canonical, path } => {
                    let source = match read_script_file(&path) {
                        Ok(s) => s,
                        Err(err) => {
                            warn!(path = %path.display(), error = %err, "failed to read watched file");
                            continue;
                        }
                    };
                    let document = match parse_document(source) {
                        Ok(d) => d,
                        Err(err) => {
                            warn!(path = %path.display(), error = %err, "failed to parse watched file");
                            continue;
                        }
                    };
                    debug!(canonical = %canonical, "watched file upserted");
                    updates.push((canonical, document));
                }
                WatchedEvent::Remove { canonical } => {
                    debug!(canonical = %canonical, "watched file removed");
                    removals.push(canonical);
                }
            }
        }

        if updates.is_empty() && removals.is_empty() {
            return;
        }

        let invalidated = {
            let mut index = self.workspace_index.lock().await;
            let mut docs = self.workspace_documents.lock().await;
            let mut invalidated: HashSet<String> = HashSet::new();
            for (canonical, document) in updates {
                invalidated.extend(index.update_document(canonical.as_str(), &document));
                docs.insert(canonical, document);
            }
            for canonical in removals {
                invalidated.extend(index.remove_document(&canonical));
                docs.remove(&canonical);
            }
            invalidated
        };
        self.evict_cache_entries(&invalidated).await;

        self.publish_open_diagnostics().await;
    }

    pub(crate) async fn fetch_config(&self) -> ConfigChange {
        let prev_base_scripts_path = self.base_scripts_path.lock().await.clone();
        let prev_files_exclude = self.files_exclude.lock().await.clone();
        let prev_additional = self.additional_script_dirs.lock().await.clone();
        let prev_auto_load = self.auto_load_mod_shared_imports.load(Ordering::Relaxed);
        let prev_diag_enabled = self.diagnostics_enabled.load(Ordering::Relaxed);

        let items = vec![
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.gameDirectory".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.logLevel".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.lineLimit".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.compactColon".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.alignMemberColons".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("files.exclude".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.additionalScriptDirectories".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.autoLoadModSharedImports".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.diagnostics.enable".to_string()),
            },
        ];
        let Ok(values) = self
            .client
            .request::<WorkspaceConfiguration>(ConfigurationParams { items })
            .await
        else {
            warn!("workspace/configuration request failed");
            return ConfigChange::default();
        };
        let mut iter = values.into_iter();
        if let Some(Value::String(path_str)) = iter.next() {
            if !path_str.is_empty() {
                *self.base_scripts_path.lock().await = Some(std::path::PathBuf::from(path_str));
            }
        }
        if let Some(Value::String(level_str)) = iter.next() {
            let new_level = level_to_u8(level_from_str(&level_str));
            self.log_level.store(new_level, Ordering::Relaxed);
            info!(level = %level_str, "log level updated");
        }
        if let Some(Value::Number(n)) = iter.next() {
            if let Some(limit) = n.as_u64() {
                log_setting_change(
                    "formatter.lineLimit",
                    self.formatter_line_limit
                        .swap(limit as u32, Ordering::Relaxed),
                    limit as u32,
                );
            }
        }
        if let Some(Value::Bool(compact)) = iter.next() {
            log_setting_change(
                "formatter.compactColon",
                self.formatter_compact_colon
                    .swap(compact, Ordering::Relaxed),
                compact,
            );
        }
        if let Some(Value::Bool(align)) = iter.next() {
            log_setting_change(
                "formatter.alignMemberColons",
                self.formatter_align_member_colons
                    .swap(align, Ordering::Relaxed),
                align,
            );
        }
        if let Some(Value::Object(map)) = iter.next() {
            let globs: Vec<String> = map
                .into_iter()
                .filter(|(_, enabled)| matches!(enabled, Value::Bool(true)))
                .map(|(glob, _)| glob)
                .collect();
            *self.files_exclude.lock().await = globs;
        }
        match iter.next() {
            Some(Value::Array(arr)) => {
                let dirs: Vec<std::path::PathBuf> = arr
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::String(s) if !s.is_empty() => Some(std::path::PathBuf::from(s)),
                        _ => None,
                    })
                    .collect();
                *self.additional_script_dirs.lock().await = dirs;
            }
            _ => {
                self.additional_script_dirs.lock().await.clear();
            }
        }
        match iter.next() {
            Some(Value::Bool(b)) => {
                self.auto_load_mod_shared_imports
                    .store(b, Ordering::Relaxed);
            }
            _ => {
                self.auto_load_mod_shared_imports
                    .store(true, Ordering::Relaxed);
            }
        }
        match iter.next() {
            Some(Value::Bool(b)) => {
                self.diagnostics_enabled.store(b, Ordering::Relaxed);
            }
            _ => {
                self.diagnostics_enabled.store(true, Ordering::Relaxed);
            }
        }

        let base_scripts_changed = *self.base_scripts_path.lock().await != prev_base_scripts_path;
        let files_exclude_changed = *self.files_exclude.lock().await != prev_files_exclude;
        let new_additional_len = self.additional_script_dirs.lock().await.len();
        let additional_changed = new_additional_len != prev_additional.len()
            || *self.additional_script_dirs.lock().await != prev_additional;
        let new_auto_load = self.auto_load_mod_shared_imports.load(Ordering::Relaxed);
        let auto_load_changed = new_auto_load != prev_auto_load;
        let new_diag_enabled = self.diagnostics_enabled.load(Ordering::Relaxed);
        let diagnostics_toggled = new_diag_enabled != prev_diag_enabled;
        if base_scripts_changed {
            trace!(setting = "gameDirectory", "setting changed");
        }
        if files_exclude_changed {
            trace!(setting = "files.exclude", "setting changed");
        }
        if additional_changed {
            trace!(
                setting = "additionalScriptDirectories",
                prev = prev_additional.len(),
                new = new_additional_len,
                "setting changed"
            );
        }
        if auto_load_changed {
            trace!(
                setting = "autoLoadModSharedImports",
                prev = prev_auto_load,
                new = new_auto_load,
                "setting changed"
            );
        }
        if diagnostics_toggled {
            trace!(
                setting = "diagnostics.enable",
                prev = prev_diag_enabled,
                new = new_diag_enabled,
                "setting changed"
            );
        }
        ConfigChange {
            needs_reindex: base_scripts_changed
                || files_exclude_changed
                || additional_changed
                || auto_load_changed,
            diagnostics_toggled,
        }
    }

    pub(crate) async fn resolve_at(&self, uri: &Url, position: Position) -> Option<Definition> {
        let documents = self.documents.lock().await;
        let document = documents.get(uri)?;
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base)
            .with_script_env(&script_env)
            .with_builtins(&self.builtins_index);
        resolve_definition(uri.as_str(), document, &db, source_position(position))
    }

    pub(crate) async fn index_base_scripts(&self) {
        let game_dir_opt = self.base_scripts_path.lock().await.clone();
        let extras = self.additional_script_dirs.lock().await.clone();
        let auto_load = self.auto_load_mod_shared_imports.load(Ordering::Relaxed);

        if game_dir_opt.is_none() && extras.is_empty() {
            let mut idx = self.base_scripts_index.lock().await;
            let mut docs = self.base_scripts_documents.lock().await;
            *idx = WorkspaceIndex::default();
            docs.clear();
            return;
        }

        if let Some(gd) = &game_dir_opt {
            if let Some(env) = parse_script_environment(&gd.join(r"bin\redscripts.ini")) {
                *self.script_env.lock().await = env;
            }
        }

        let segments = build_index_segments(game_dir_opt.as_deref(), &extras, auto_load);
        let segments_count = segments.len();
        let total_start = Instant::now();

        let join_result = tokio::task::spawn_blocking(move || {
            let mut new_index = WorkspaceIndex::default();
            let mut new_docs: HashMap<String, ParsedDocument> = HashMap::new();
            let mut total_indexed: usize = 0;

            for (label, root, is_auto) in &segments {
                let seg_start = Instant::now();
                let Ok(files) = collect_witcherscript_files(std::slice::from_ref(root), &[]) else {
                    warn!(label, path = %root.display(), "failed to collect script files");
                    continue;
                };
                let parsed: Vec<(String, ParsedDocument)> = files
                    .par_iter()
                    .filter_map(|path| {
                        let source = read_script_file(path)
                            .map_err(|e| warn!(path = %path.display(), error = %e, "failed to read base script"))
                            .ok()?;
                        let document = parse_document(source)
                            .map_err(|e| warn!(path = %path.display(), error = %e, "failed to parse base script"))
                            .ok()?;
                        let uri = Url::from_file_path(path)
                            .map_err(|_| warn!(path = %path.display(), "failed to convert base script path to URI"))
                            .ok()?;
                        Some((uri.to_string(), document))
                    })
                    .collect();

                let count = parsed.len();
                total_indexed += count;
                for (uri, doc) in parsed {
                    new_index.update_document(uri.as_str(), &doc);
                    new_docs.insert(uri, doc);
                }
                let elapsed_ms = seg_start.elapsed().as_millis();
                if *is_auto {
                    info!(
                        label,
                        path = %root.display(),
                        indexed = count,
                        elapsed_ms,
                        auto_loaded = true,
                        "[auto-detected] indexed modSharedImports"
                    );
                } else {
                    info!(
                        label,
                        path = %root.display(),
                        indexed = count,
                        elapsed_ms,
                        "indexed scripts segment"
                    );
                }
            }
            (new_index, new_docs, total_indexed)
        })
        .await;

        let (new_index, new_docs, total_indexed) = match join_result {
            Ok(triple) => triple,
            Err(err) => {
                error!(error = %err, "base scripts indexing task panicked");
                return;
            }
        };

        {
            let mut idx = self.base_scripts_index.lock().await;
            let mut docs = self.base_scripts_documents.lock().await;
            *idx = new_index;
            *docs = new_docs;
        }

        let elapsed_ms = total_start.elapsed().as_millis();
        info!(
            segments = segments_count,
            indexed = total_indexed,
            elapsed_ms,
            elapsed_secs = format!("{:.1}", elapsed_ms as f32 / 1000.0),
            "base scripts indexed"
        );

        self.publish_open_diagnostics().await;
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigChange;

    #[test]
    fn config_change_default_is_no_op() {
        let c = ConfigChange::default();
        struct Case {
            name: &'static str,
            change: ConfigChange,
            expect_any_action: bool,
        }
        let cases = [
            Case {
                name: "default → nothing to do",
                change: c,
                expect_any_action: false,
            },
            Case {
                name: "reindex only",
                change: ConfigChange {
                    needs_reindex: true,
                    diagnostics_toggled: false,
                },
                expect_any_action: true,
            },
            Case {
                name: "diagnostics toggle only",
                change: ConfigChange {
                    needs_reindex: false,
                    diagnostics_toggled: true,
                },
                expect_any_action: true,
            },
            Case {
                name: "both at once",
                change: ConfigChange {
                    needs_reindex: true,
                    diagnostics_toggled: true,
                },
                expect_any_action: true,
            },
        ];
        for c in cases {
            let any = c.change.needs_reindex || c.change.diagnostics_toggled;
            assert_eq!(
                any, c.expect_any_action,
                "case {}: action predicate wrong",
                c.name
            );
        }
    }
}
