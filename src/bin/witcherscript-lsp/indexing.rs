use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;

use rayon::prelude::*;
use serde_json::Value;
use tower_lsp::lsp_types::{ConfigurationItem, Position, Url};
use tracing::{debug, error, info, trace, warn};
use witcherscript_parser::diagnostics::collect_duplicate_symbol_diagnostics;
use witcherscript_parser::document::{parse_document, ParsedDocument};
use witcherscript_parser::files::collect_witcherscript_files;
use witcherscript_parser::resolve::{resolve_definition, Definition, SymbolDb, WorkspaceIndex};
use witcherscript_parser::script_env::parse_script_environment;

use crate::backend::Backend;
use crate::convert::{
    canonical_uri, lsp_diagnostics, lsp_workspace_diagnostic, read_script_file, source_position,
};

fn log_setting_change<T: PartialEq + std::fmt::Display>(setting: &str, prev: T, new: T) {
    if prev != new {
        trace!(setting, prev = %prev, new = %new, "setting changed");
    }
}

pub(crate) fn has_top_level_func_body(document: &ParsedDocument) -> bool {
    let root = document.tree.root_node();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor).filter(|c| c.is_named()) {
        if child.kind() == "func_decl" {
            let mut inner = child.walk();
            if child.children(&mut inner).any(|c| c.kind() == "func_block") {
                return true;
            }
        }
    }
    false
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

pub(crate) fn index_open_document(
    index: &mut WorkspaceIndex,
    uri: &Url,
    document: &ParsedDocument,
) {
    // index_workspace keys this file under the canonical spelling; drop that copy so it is not indexed twice.
    if let Some(canonical) = canonical_uri(uri) {
        if canonical != uri.as_str() {
            index.remove_document(&canonical);
        }
    }
    index.update_document(uri.as_str(), document);
}
use crate::logging::{level_from_str, level_to_u8};

impl Backend {
    pub(crate) async fn update_open_document(&self, uri: Url, text: String) {
        match parse_document(text) {
            Ok(document) => {
                {
                    let mut index = self.workspace_index.lock().await;
                    index_open_document(&mut index, &uri, &document);
                }
                self.documents.lock().await.insert(uri.clone(), document);
                self.publish_open_diagnostics().await;
            }
            Err(err) => {
                error!(uri = %uri, error = %err, "failed to parse document");
            }
        }
    }

    async fn publish_open_diagnostics(&self) {
        let start = Instant::now();
        let dup_by_uri = {
            let index = self.workspace_index.lock().await;
            collect_duplicate_symbol_diagnostics(&index)
        };
        let collect_us = start.elapsed().as_micros();
        let documents = self.documents.lock().await;
        let mut published = self.published_diagnostics.lock().await;
        let mut republished = 0;
        for (uri, document) in documents.iter() {
            let mut diagnostics = lsp_diagnostics(document);
            if let Some(dups) = dup_by_uri.get(uri.as_str()) {
                diagnostics.extend(dups.iter().map(lsp_workspace_diagnostic));
            }
            if published.get(uri) == Some(&diagnostics) {
                continue;
            }
            self.client
                .publish_diagnostics(uri.clone(), diagnostics.clone(), None)
                .await;
            published.insert(uri.clone(), diagnostics);
            republished += 1;
        }
        trace!(
            open_documents = documents.len(),
            flagged_uris = dup_by_uri.len(),
            republished,
            collect_us,
            total_us = start.elapsed().as_micros(),
            "recomputed workspace diagnostics for open documents"
        );
    }

    pub(crate) async fn index_workspace(&self) {
        let roots = self.workspace_roots.lock().await.clone();
        if roots.is_empty() {
            return;
        }
        let exclude_globs = self.files_exclude.lock().await.clone();

        info!(roots = ?roots, "indexing workspace");
        let start = Instant::now();

        let Ok(files) = collect_witcherscript_files(&roots, &exclude_globs) else {
            warn!("failed to collect workspace files");
            return;
        };

        let file_count = files.len();

        let parsed: Vec<(String, ParsedDocument)> = files
            .iter()
            .filter_map(|path| {
                let source = fs::read_to_string(path)
                    .map_err(|_| warn!(path = %path.display(), "failed to read workspace file"))
                    .ok()?;
                let document = parse_document(source)
                    .map_err(|_| warn!(path = %path.display(), "failed to parse workspace file"))
                    .ok()?;
                let uri = Url::from_file_path(path)
                    .map_err(|_| warn!(path = %path.display(), "failed to convert path to URI"))
                    .ok()?;
                debug!(uri = %uri, "indexed workspace file");
                Some((uri.to_string(), document))
            })
            .collect();

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

    pub(crate) async fn fetch_config(&self) -> bool {
        let prev_base_scripts_path = self.base_scripts_path.lock().await.clone();
        let prev_files_exclude = self.files_exclude.lock().await.clone();
        let prev_additional = self.additional_script_dirs.lock().await.clone();
        let prev_auto_load = self.auto_load_mod_shared_imports.load(Ordering::Relaxed);

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
        ];
        let Ok(values) = self.client.configuration(items).await else {
            warn!("workspace/configuration request failed");
            return false;
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

        let base_scripts_changed = *self.base_scripts_path.lock().await != prev_base_scripts_path;
        let files_exclude_changed = *self.files_exclude.lock().await != prev_files_exclude;
        let new_additional_len = self.additional_script_dirs.lock().await.len();
        let additional_changed = new_additional_len != prev_additional.len()
            || *self.additional_script_dirs.lock().await != prev_additional;
        let new_auto_load = self.auto_load_mod_shared_imports.load(Ordering::Relaxed);
        let auto_load_changed = new_auto_load != prev_auto_load;
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
        base_scripts_changed || files_exclude_changed || additional_changed || auto_load_changed
    }

    pub(crate) async fn resolve_at(&self, uri: &Url, position: Position) -> Option<Definition> {
        let documents = self.documents.lock().await;
        let document = documents.get(uri)?;
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base).with_script_env(&script_env);
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

        let mut new_index = WorkspaceIndex::default();
        let mut new_docs: HashMap<String, ParsedDocument> = HashMap::new();
        let total_start = Instant::now();
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
                    let source = read_script_file(path).ok()?;
                    let document = parse_document(source).ok()?;
                    let uri = Url::from_file_path(path).ok()?;
                    Some((uri.to_string(), document))
                })
                .collect();

            if *label == "modSharedImports" {
                if let Some((bad_uri, _)) = parsed.iter().find(|(_, d)| has_top_level_func_body(d))
                {
                    warn!(
                        uri = %bad_uri,
                        path = %root.display(),
                        auto_loaded = true,
                        "[auto-detected] modSharedImports has a top-level function with a body; skipping"
                    );
                    continue;
                }
            }

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

        {
            let mut idx = self.base_scripts_index.lock().await;
            let mut docs = self.base_scripts_documents.lock().await;
            *idx = new_index;
            *docs = new_docs;
        }

        let elapsed_ms = total_start.elapsed().as_millis();
        info!(
            segments = segments.len(),
            indexed = total_indexed,
            elapsed_ms,
            elapsed_secs = format!("{:.1}", elapsed_ms as f32 / 1000.0),
            "base scripts indexed"
        );
    }
}
