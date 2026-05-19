use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lsp_types::{Position, Url};
use rayon::prelude::*;
use tracing::{debug, error, info, warn};
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::files::{collect_witcherscript_files, read_script_file};
use witcherscript_language::resolve::{resolve_definition, Definition, WorkspaceIndex};
use witcherscript_language::script_env::parse_script_environment;

use crate::backend::Backend;
use crate::convert::{canonical_uri, source_position};

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

    pub(crate) async fn index_base_scripts(&self) {
        let game_dir_opt = self.base_scripts_path.lock().await.clone();
        let extras = self.additional_script_dirs.lock().await.clone();
        let auto_load = self.config.load().auto_load_mod_shared_imports;

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
