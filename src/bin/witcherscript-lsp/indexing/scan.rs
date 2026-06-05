use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use lsp_types::Url;
use rayon::prelude::*;
use tracing::{debug, error, info, trace, warn};
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::files::{canonical_uri, collect_witcherscript_files, read_text_file};
use witcherscript_language::resolve::WorkspaceIndex;
use witcherscript_language::script_env::parse_script_environment;

use crate::backend::Backend;
use crate::file_scope::FileScope;

use super::helpers::{build_index_segments, index_open_document, legacy_base_replacements};

fn parse_script_files(files: &[PathBuf], label: &str) -> Vec<(String, ParsedDocument)> {
    files
        .par_iter()
        .filter_map(|path| {
            let source = read_text_file(path)
                .map_err(|e| warn!(path = %path.display(), label, error = %e, "failed to read script"))
                .ok()?;
            let document = parse_document(source)
                .map_err(|e| warn!(path = %path.display(), label, error = %e, "failed to parse script"))
                .ok()?;
            let uri = Url::from_file_path(path)
                .map_err(|_| warn!(path = %path.display(), label, "failed to convert script path to URI"))
                .ok()?;
            Some((uri.to_string(), document))
        })
        .collect()
}

impl Backend {
    pub(crate) fn is_base_script_uri(&self, uri: &Url) -> bool {
        matches!(self.file_scope_of(uri), FileScope::AdditionalBase)
    }

    fn merge_open_base_documents(&self) {
        let snap = self.snapshot();
        let base_uris: Vec<Url> = snap
            .documents
            .keys()
            .filter(|uri| self.is_base_script_uri(uri))
            .cloned()
            .collect();
        if base_uris.is_empty() {
            return;
        }
        let docs = snap.documents.clone();
        self.publish_compilation(|builder| {
            let idx = builder.base_scripts_index_mut();
            for uri in &base_uris {
                if let Some(doc) = docs.get(uri) {
                    index_open_document(idx, uri, doc.as_ref());
                }
            }
        });
    }

    pub(crate) async fn index_workspace(&self) {
        let roots = self.workspace_roots.lock().clone();
        if roots.is_empty() {
            self.workspace_known_files.lock().clear();
            return;
        }
        let exclude_globs = self.files_exclude.lock().clone();

        info!(op = "index_workspace", roots = ?roots, "start");
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
            trace!(op = "index_workspace", file_count, files = ?files, "workspace files");
            let known_uris: HashSet<String> = files
                .iter()
                .filter_map(|p| Url::from_file_path(p).ok())
                .map(|u| u.to_string())
                .collect();
            let parsed: Vec<(String, ParsedDocument)> = files
                .par_iter()
                .filter_map(|path| {
                    let source = read_text_file(path)
                        .map_err(|e| warn!(path = %path.display(), error = %e, "failed to read workspace file"))
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
            Some((file_count, known_uris, parsed))
        })
        .await;

        let (file_count, known_uris, parsed) = match join_result {
            Ok(Some(v)) => v,
            Ok(None) => return,
            Err(err) => {
                error!(error = %err, "workspace indexing task panicked");
                return;
            }
        };

        *self.workspace_known_files.lock() = known_uris;

        // Skip files the editor has open; update_open_document keeps them indexed under the client spelling.
        let open_canonical: HashSet<String> = self
            .snapshot()
            .documents
            .keys()
            .map(canonical_uri)
            .collect();

        let filtered: Vec<(String, Arc<ParsedDocument>)> = parsed
            .into_iter()
            .filter(|(uri, _)| !open_canonical.contains(uri))
            .map(|(uri, doc)| (uri, Arc::new(doc)))
            .collect();
        let indexed = filtered.len();
        self.publish_compilation(|builder| {
            let index = builder.workspace_index_mut();
            index.begin_bulk_catalog_update();
            for (uri, document) in &filtered {
                index.update_document(uri.as_str(), document.as_ref());
            }
            index.end_bulk_catalog_update();
            let docs = builder.workspace_documents_mut();
            for (uri, document) in filtered {
                docs.insert(uri, document);
            }
        });

        info!(
            op = "index_workspace",
            indexed,
            file_count,
            elapsed_ms = start.elapsed().as_millis(),
            "complete"
        );
    }

    pub(crate) async fn index_base_scripts(&self) {
        info!(op = "index_base_scripts", "start");
        let game_dir_opt = self.base_scripts_path.lock().clone();
        let extras = self.additional_script_dirs.lock().clone();
        let legacy_dirs = self.effective_legacy_dirs();

        if game_dir_opt.is_none() && extras.is_empty() && legacy_dirs.is_empty() {
            self.publish_compilation(|builder| {
                builder.set_base_scripts_index(WorkspaceIndex::default());
                builder.set_base_scripts_documents(HashMap::new());
                builder.set_suppressed_base_uris(HashSet::new());
            });
            self.legacy_replacements.lock().clear();
            self.rebuild_filtered_base_catalogs();
            self.prune_stale_legacy_workspace_files(&HashSet::new());
            self.publish_legacy_script_status();
            self.publish_file_scope_status();
            info!(
                op = "index_base_scripts",
                reason = "no_paths_configured",
                "complete",
            );
            return;
        }

        if let Some(gd) = &game_dir_opt {
            if let Some(env) = parse_script_environment(&gd.join(r"bin\redscripts.ini")) {
                self.publish_compilation(|builder| {
                    *builder.script_env_mut() = env;
                });
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

        let base_segments = build_index_segments(game_dir_opt.as_deref(), &extras_filtered);
        let base_segments_count = base_segments.len();
        let total_start = Instant::now();
        let legacy_dirs_for_task = legacy_dirs_valid.clone();

        let join_result = tokio::task::spawn_blocking(move || {
            let mut base_index = WorkspaceIndex::default();
            let mut base_docs: HashMap<String, ParsedDocument> = HashMap::new();
            let mut base_total: usize = 0;
            base_index.begin_bulk_catalog_update();

            for (label, root) in &base_segments {
                let seg_start = Instant::now();
                let Ok(files) = collect_witcherscript_files(std::slice::from_ref(root), &[]) else {
                    warn!(label, path = %root.display(), "failed to collect script files");
                    continue;
                };
                let parsed = parse_script_files(&files, "base");

                let count = parsed.len();
                base_total += count;
                for (uri, doc) in parsed {
                    base_index.update_document(uri.as_str(), &doc);
                    base_docs.insert(uri, doc);
                }
                let elapsed_ms = seg_start.elapsed().as_millis();
                info!(
                    op = "index_base_scripts",
                    segment = "base",
                    label,
                    path = %root.display(),
                    indexed = count,
                    elapsed_ms,
                    "indexed scripts segment"
                );
            }

            let mut legacy_parsed: Vec<(String, ParsedDocument)> = Vec::new();
            for dir in &legacy_dirs_for_task {
                let seg_start = Instant::now();
                let Ok(files) = collect_witcherscript_files(std::slice::from_ref(dir), &[]) else {
                    warn!(path = %dir.display(), "failed to collect legacy script files");
                    continue;
                };
                let parsed = parse_script_files(&files, "legacy");
                let count = parsed.len();
                let elapsed_ms = seg_start.elapsed().as_millis();
                info!(
                    op = "index_base_scripts",
                    segment = "legacy",
                    label = "legacyScriptDirectory",
                    path = %dir.display(),
                    indexed = count,
                    elapsed_ms,
                    "indexed legacy scripts segment"
                );
                legacy_parsed.extend(parsed);
            }

            let base_uris: Vec<String> = base_docs.keys().cloned().collect();
            let legacy_uris: Vec<String> =
                legacy_parsed.iter().map(|(uri, _)| uri.clone()).collect();
            let (suppressed_base, legacy_replacements) =
                legacy_base_replacements(&base_uris, &legacy_uris);
            base_index.end_bulk_catalog_update();
            let matched_count = suppressed_base.len();
            let legacy_total = legacy_parsed.len();

            (
                base_index,
                base_docs,
                base_total,
                legacy_parsed,
                legacy_total,
                matched_count,
                legacy_replacements,
                suppressed_base,
            )
        })
        .await;

        let (
            base_new_index,
            base_new_docs,
            base_total,
            legacy_parsed,
            legacy_total,
            matched_count,
            legacy_replacements,
            suppressed_base,
        ) = match join_result {
            Ok(tuple) => tuple,
            Err(err) => {
                error!(error = %err, "base scripts indexing task panicked");
                return;
            }
        };

        let base_new_docs_arc: HashMap<String, Arc<ParsedDocument>> = base_new_docs
            .into_iter()
            .map(|(uri, doc)| (uri, Arc::new(doc)))
            .collect();
        self.publish_compilation(|builder| {
            builder.set_base_scripts_index(base_new_index);
            builder.set_base_scripts_documents(base_new_docs_arc);
            builder.set_suppressed_base_uris(suppressed_base);
        });
        *self.legacy_replacements.lock() = legacy_replacements;
        self.merge_open_base_documents();
        self.rebuild_filtered_base_catalogs();

        let invalidated = self.sync_legacy_workspace_from_parsed(legacy_parsed);
        self.evict_cache_entries(&invalidated);

        let elapsed_ms = total_start.elapsed().as_millis();
        info!(
            op = "index_base_scripts",
            segments = base_segments_count,
            indexed = base_total,
            legacy_indexed = legacy_total,
            base_replaced_by_legacy = matched_count,
            elapsed_ms,
            elapsed_secs = format!("{:.1}", elapsed_ms as f32 / 1000.0),
            "complete"
        );

        self.publish_legacy_script_status();
        self.publish_file_scope_status();
    }
}
