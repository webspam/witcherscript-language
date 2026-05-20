use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lsp_types::{Position, Url};
use rayon::prelude::*;
use tracing::{debug, error, info, warn};
use witcherscript_language::diagnostics::{basename_of, relative_from_scripts};
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::files::{collect_witcherscript_files, read_script_file};
use witcherscript_language::resolve::{resolve_definition, Definition, WorkspaceIndex};
use witcherscript_language::script_env::parse_script_environment;

use crate::backend::Backend;
use crate::convert::{canonical_uri, source_position};

pub(crate) fn legacy_replaces_base(base_uri: &str, legacy_uri: &str) -> bool {
    let Some(tail) = relative_from_scripts(base_uri) else {
        return false;
    };
    let needle = format!("/{tail}");
    legacy_uri.ends_with(&needle)
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

impl Backend {
    #[tracing::instrument(skip(self, text), fields(uri = %uri, bytes = text.len()), level = "debug")]
    pub(crate) async fn update_open_document(&self, uri: Url, text: String) {
        let parsed = tracing::debug_span!("parse_document").in_scope(|| parse_document(text));
        match parsed {
            Ok(document) => {
                // Indexing an opened base script as a workspace declaration duplicates its override.
                let invalidated = if self.is_base_script_uri(&uri).await {
                    let mut index = self.base_scripts_index.lock().await;
                    index_open_document(&mut index, &uri, &document)
                } else {
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

    async fn is_base_script_uri(&self, uri: &Url) -> bool {
        let Ok(path) = uri.to_file_path() else {
            return false;
        };
        if self
            .legacy_script_dirs
            .lock()
            .await
            .iter()
            .any(|dir| path.starts_with(dir))
        {
            return false;
        }
        if let Some(game_dir) = self.base_scripts_path.lock().await.as_ref() {
            if path.starts_with(game_dir) {
                return true;
            }
        }
        self.additional_script_dirs
            .lock()
            .await
            .iter()
            .any(|dir| path.starts_with(dir))
    }

    pub(super) async fn evict_cache_entries(&self, uris: &HashSet<String>) {
        if uris.is_empty() {
            return;
        }
        let mut cache = self.cst_diag_cache.lock().await;
        cache.retain(|url, _| !uris.contains(url.as_str()));
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

    pub(crate) async fn resolve_at(&self, uri: &Url, position: Position) -> Option<Definition> {
        let documents = self.documents.lock().await;
        let document = documents.get(uri)?;
        let handles = self.db_handles().await;
        let db = handles.db();
        resolve_definition(uri.as_str(), document, &db, source_position(position))
    }

    // A deleted legacy file has no other removal path, so each run drops the previous set.
    async fn reconcile_legacy_workspace_files(&self, parsed: Vec<(String, ParsedDocument)>) {
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock().await;
            documents.keys().filter_map(canonical_uri).collect()
        };
        let mut ws_idx = self.workspace_index.lock().await;
        let mut ws_docs = self.workspace_documents.lock().await;
        let mut tracked = self.legacy_indexed_uris.lock().await;
        for uri in tracked.drain() {
            ws_idx.remove_document(&uri);
            ws_docs.remove(&uri);
        }
        for (uri, doc) in parsed {
            if open_canonical.contains(&uri) {
                continue;
            }
            ws_idx.update_document(uri.as_str(), &doc);
            ws_docs.insert(uri.clone(), doc);
            tracked.insert(uri);
        }
    }

    pub(crate) async fn index_base_scripts(&self) {
        let game_dir_opt = self.base_scripts_path.lock().await.clone();
        let extras = self.additional_script_dirs.lock().await.clone();
        let legacy_dirs = self.legacy_script_dirs.lock().await.clone();
        let auto_load = self.config.load().auto_load_mod_shared_imports;

        if game_dir_opt.is_none() && extras.is_empty() && legacy_dirs.is_empty() {
            {
                let mut idx = self.base_scripts_index.lock().await;
                let mut docs = self.base_scripts_documents.lock().await;
                *idx = WorkspaceIndex::default();
                docs.clear();
            }
            self.reconcile_legacy_workspace_files(Vec::new()).await;
            self.publish_open_diagnostics().await;
            return;
        }

        if let Some(gd) = &game_dir_opt {
            if let Some(env) = parse_script_environment(&gd.join(r"bin\redscripts.ini")) {
                *self.script_env.lock().await = env;
            }
        }

        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let legacy_dirs_valid: Vec<PathBuf> = legacy_dirs
            .iter()
            .filter(|p| {
                if !p.is_dir() {
                    warn!(path = %p.display(), "legacyScriptDirectories entry is not a directory; skipping");
                    false
                } else {
                    true
                }
            })
            .cloned()
            .collect();
        let legacy_canon: HashSet<PathBuf> = legacy_dirs_valid.iter().map(|p| canon(p)).collect();
        let extras_filtered: Vec<PathBuf> = extras
            .into_iter()
            .filter(|p| {
                if legacy_canon.contains(&canon(p)) {
                    warn!(
                        path = %p.display(),
                        "path appears in both additionalScriptDirectories and legacyScriptDirectories; treating as legacy"
                    );
                    false
                } else {
                    true
                }
            })
            .collect();

        let base_segments =
            build_index_segments(game_dir_opt.as_deref(), &extras_filtered, auto_load);
        let base_segments_count = base_segments.len();
        let total_start = Instant::now();
        let legacy_dirs_for_task = legacy_dirs_valid.clone();

        let join_result = tokio::task::spawn_blocking(move || {
            let mut base_index = WorkspaceIndex::default();
            let mut base_docs: HashMap<String, ParsedDocument> = HashMap::new();
            let mut base_total: usize = 0;

            for (label, root, is_auto) in &base_segments {
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
                base_total += count;
                for (uri, doc) in parsed {
                    base_index.update_document(uri.as_str(), &doc);
                    base_docs.insert(uri, doc);
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

            let mut legacy_parsed: Vec<(String, ParsedDocument)> = Vec::new();
            for dir in &legacy_dirs_for_task {
                let seg_start = Instant::now();
                let Ok(files) = collect_witcherscript_files(std::slice::from_ref(dir), &[]) else {
                    warn!(path = %dir.display(), "failed to collect legacy script files");
                    continue;
                };
                let parsed: Vec<(String, ParsedDocument)> = files
                    .par_iter()
                    .filter_map(|path| {
                        let source = read_script_file(path)
                            .map_err(|e| warn!(path = %path.display(), error = %e, "failed to read legacy script"))
                            .ok()?;
                        let document = parse_document(source)
                            .map_err(|e| warn!(path = %path.display(), error = %e, "failed to parse legacy script"))
                            .ok()?;
                        let uri = Url::from_file_path(path)
                            .map_err(|_| warn!(path = %path.display(), "failed to convert legacy script path to URI"))
                            .ok()?;
                        Some((uri.to_string(), document))
                    })
                    .collect();
                let count = parsed.len();
                let elapsed_ms = seg_start.elapsed().as_millis();
                info!(
                    label = "legacyScriptDirectory",
                    path = %dir.display(),
                    indexed = count,
                    elapsed_ms,
                    "indexed legacy scripts segment"
                );
                legacy_parsed.extend(parsed);
            }

            // A legacy file can only replace a base script with the same filename.
            let mut base_by_basename: HashMap<&str, Vec<&String>> = HashMap::new();
            for base_uri in base_docs.keys() {
                if let Some(name) = basename_of(base_uri) {
                    base_by_basename.entry(name).or_default().push(base_uri);
                }
            }
            let mut skip_base: HashSet<String> = HashSet::new();
            for (legacy_uri, _) in &legacy_parsed {
                let Some(candidates) = basename_of(legacy_uri)
                    .and_then(|name| base_by_basename.get(name))
                else {
                    continue;
                };
                for base_uri in candidates {
                    if legacy_replaces_base(base_uri, legacy_uri) {
                        skip_base.insert((*base_uri).clone());
                    }
                }
            }
            for skip_uri in &skip_base {
                base_index.remove_document(skip_uri);
                base_docs.remove(skip_uri);
            }
            let matched_count = skip_base.len();
            let legacy_total = legacy_parsed.len();

            (
                base_index,
                base_docs,
                base_total,
                legacy_parsed,
                legacy_total,
                matched_count,
            )
        })
        .await;

        let (base_new_index, base_new_docs, base_total, legacy_parsed, legacy_total, matched_count) =
            match join_result {
                Ok(tuple) => tuple,
                Err(err) => {
                    error!(error = %err, "base scripts indexing task panicked");
                    return;
                }
            };

        {
            let mut idx = self.base_scripts_index.lock().await;
            let mut docs = self.base_scripts_documents.lock().await;
            *idx = base_new_index;
            *docs = base_new_docs;
        }

        self.reconcile_legacy_workspace_files(legacy_parsed).await;

        let elapsed_ms = total_start.elapsed().as_millis();
        info!(
            segments = base_segments_count,
            indexed = base_total,
            legacy_indexed = legacy_total,
            base_replaced_by_legacy = matched_count,
            elapsed_ms,
            elapsed_secs = format!("{:.1}", elapsed_ms as f32 / 1000.0),
            "base scripts indexed"
        );

        self.publish_open_diagnostics().await;
    }
}
