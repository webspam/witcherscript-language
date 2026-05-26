use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lsp_types::{Position, Url};
use rayon::prelude::*;
use tracing::{debug, error, info, warn};
use witcherscript_language::diagnostics::{basename_of, relative_from_scripts};
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::files::{canonical_uri, collect_witcherscript_files, read_script_file};
use witcherscript_language::resolve::{resolve_definition, Definition, WorkspaceIndex};
use witcherscript_language::script_env::parse_script_environment;

use crate::backend::Backend;
use crate::convert::source_position;
use crate::file_scope::{classify_file_scope, FileScope};

fn path_to_canonical_uri(path: &Path) -> Option<String> {
    Url::from_file_path(path)
        .ok()
        .and_then(|u| canonical_uri(&u))
}

pub(crate) fn legacy_replaces_base(base_uri: &str, legacy_uri: &str) -> bool {
    let Some(tail) = relative_from_scripts(base_uri) else {
        return false;
    };
    let needle = format!("/{tail}");
    legacy_uri.ends_with(&needle)
}

pub(crate) fn legacy_base_replacements(
    base_uris: &[String],
    legacy_uris: &[String],
) -> (HashSet<String>, HashMap<String, String>) {
    let mut base_by_basename: HashMap<&str, Vec<&String>> = HashMap::new();
    for base_uri in base_uris {
        if let Some(name) = basename_of(base_uri) {
            base_by_basename.entry(name).or_default().push(base_uri);
        }
    }
    let mut skip_base: HashSet<String> = HashSet::new();
    let mut replacements: HashMap<String, String> = HashMap::new();
    for legacy_uri in legacy_uris {
        let Some(candidates) = basename_of(legacy_uri).and_then(|name| base_by_basename.get(name))
        else {
            continue;
        };
        let canonical = Url::parse(legacy_uri)
            .ok()
            .and_then(|u| canonical_uri(&u))
            .unwrap_or_else(|| legacy_uri.clone());
        for base_uri in candidates {
            if legacy_replaces_base(base_uri, legacy_uri) {
                skip_base.insert((*base_uri).clone());
                if let Some(rel) = relative_from_scripts(base_uri) {
                    replacements.insert(canonical.clone(), rel.to_string());
                }
            }
        }
    }
    (skip_base, replacements)
}

pub(crate) fn build_index_segments(
    game_dir: Option<&Path>,
    extras: &[PathBuf],
) -> Vec<(&'static str, PathBuf)> {
    let mut segments: Vec<(&'static str, PathBuf)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());

    if let Some(gd) = game_dir {
        let base = gd.join(r"content\content0\scripts");
        if seen.insert(canon(&base)) {
            segments.push(("gameDirectory", base));
        }
    }

    for extra in extras {
        if !extra.is_dir() {
            warn!(path = %extra.display(), "additionalScriptDirectories entry is not a directory; skipping");
            continue;
        }
        if seen.insert(canon(extra)) {
            segments.push(("additionalScriptDirectory", extra.clone()));
        }
    }

    segments
}

// modSharedImports ships replacement scripts, so it is indexed as a legacy
// script dir rather than a base overlay.
pub(crate) fn mod_shared_imports_dir(game_dir: &Path) -> Option<PathBuf> {
    let msi = game_dir.join(r"Mods\modSharedImports");
    msi.is_dir().then_some(msi)
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

pub(crate) fn remove_document_all_spellings(
    index: &mut WorkspaceIndex,
    uri: &Url,
) -> HashSet<String> {
    let mut invalidated = index.remove_document(uri.as_str());
    if let Some(canonical) = canonical_uri(uri) {
        if canonical != uri.as_str() {
            invalidated.extend(index.remove_document(&canonical));
        }
    }
    invalidated
}

// A closed file reverts to disk content, re-keyed from the open spelling to canonical.
fn reindex_into(
    index: &mut WorkspaceIndex,
    docs: &mut HashMap<String, ParsedDocument>,
    client_uri: &str,
    canonical: &str,
    parsed: Option<ParsedDocument>,
) -> HashSet<String> {
    let mut invalidated = HashSet::new();
    if client_uri != canonical {
        invalidated.extend(index.remove_document(client_uri));
    }
    match parsed {
        Some(document) => {
            invalidated.extend(index.update_document(canonical, &document));
            docs.insert(canonical.to_string(), document);
        }
        None => {
            invalidated.extend(index.remove_document(canonical));
            docs.remove(canonical);
        }
    }
    invalidated
}

impl Backend {
    // A workspace-folder or config change reroutes open docs now, not on next keystroke.
    #[tracing::instrument(skip(self), level = "debug")]
    pub(crate) fn reindex_open_documents(&self) {
        let documents = self.documents.lock();
        if documents.is_empty() {
            return;
        }
        let roots = self.workspace_roots.lock().clone();
        let legacy_dirs = self.effective_legacy_dirs();
        let game_dir = self.base_scripts_path.lock().clone();
        let additional = self.additional_script_dirs.lock().clone();
        let replacements = self.legacy_replacements.lock().clone();
        let scopes: Vec<(Url, FileScope)> = documents
            .keys()
            .map(|uri| {
                (
                    uri.clone(),
                    classify_file_scope(
                        uri,
                        &roots,
                        &legacy_dirs,
                        &replacements,
                        game_dir.as_deref(),
                        &additional,
                    ),
                )
            })
            .collect();

        let mut invalidated: HashSet<String> = HashSet::new();
        {
            let mut workspace = self.workspace_index.lock();
            let mut loose = self.loose_index.lock();
            let mut base = self.base_scripts_index.lock();
            for (uri, scope) in &scopes {
                let Some(document) = documents.get(uri) else {
                    continue;
                };
                invalidated.extend(remove_document_all_spellings(&mut workspace, uri));
                invalidated.extend(remove_document_all_spellings(&mut loose, uri));
                invalidated.extend(remove_document_all_spellings(&mut base, uri));
                let target: &mut WorkspaceIndex = match scope {
                    FileScope::AdditionalBase => &mut base,
                    FileScope::OutOfScope | FileScope::SingleFile => &mut loose,
                    _ => &mut workspace,
                };
                invalidated.extend(target.update_document(uri.as_str(), document));
                invalidated.insert(uri.to_string());
            }
        }
        drop(documents);
        self.evict_cache_entries(&invalidated);
    }

    // Closing a non-loose file reverts it from buffer to on-disk content in the workspace/base index.
    pub(crate) fn reindex_closed_file(&self, uri: &Url) {
        let canonical = canonical_uri(uri).unwrap_or_else(|| uri.to_string());
        let is_base = self.is_base_script_uri(uri);
        let parsed = uri
            .to_file_path()
            .ok()
            .and_then(|path| read_script_file(&path).ok())
            .and_then(|text| parse_document(text).ok());

        let invalidated = if is_base {
            let mut index = self.base_scripts_index.lock();
            let mut docs = self.base_scripts_documents.lock();
            reindex_into(&mut index, &mut docs, uri.as_str(), &canonical, parsed)
        } else {
            let mut index = self.workspace_index.lock();
            let mut docs = self.workspace_documents.lock();
            reindex_into(&mut index, &mut docs, uri.as_str(), &canonical, parsed)
        };
        self.evict_cache_entries(&invalidated);
    }

    #[tracing::instrument(skip(self, text), fields(uri = %uri, bytes = text.len()), level = "debug")]
    pub(crate) fn update_open_document(&self, uri: Url, text: String) {
        let parsed = tracing::debug_span!("parse_document").in_scope(|| parse_document(text));
        let document = match parsed {
            Ok(document) => document,
            Err(err) => {
                error!(uri = %uri, error = %err, "failed to parse document");
                return;
            }
        };

        let scope = self.file_scope_of(&uri);
        let mut invalidated = HashSet::new();
        // A config change can move a file between scopes; drop every stale copy first.
        for index in [
            &self.workspace_index,
            &self.base_scripts_index,
            &self.loose_index,
        ] {
            let mut index = index.lock();
            invalidated.extend(remove_document_all_spellings(&mut index, &uri));
        }

        let target = match scope {
            FileScope::AdditionalBase => &self.base_scripts_index,
            FileScope::OutOfScope | FileScope::SingleFile => &self.loose_index,
            _ => &self.workspace_index,
        };
        {
            let mut index = target.lock();
            invalidated.extend(index.update_document(uri.as_str(), &document));
        }

        self.evict_cache_entries(&invalidated);
        self.documents.lock().insert(uri.clone(), document);
        self.publish_open_diagnostics();
    }

    // Auto-detected modSharedImports counts as legacy without being in the setting.
    pub(crate) fn effective_legacy_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = self.legacy_script_dirs.lock().clone();
        if self.config.load().auto_load_mod_shared_imports {
            if let Some(gd) = self.base_scripts_path.lock().as_ref() {
                if let Some(msi) = mod_shared_imports_dir(gd) {
                    if !dirs.contains(&msi) {
                        dirs.push(msi);
                    }
                }
            }
        }
        for dir in self.manifest_legacy_dirs.lock().values() {
            if !dirs.contains(dir) {
                dirs.push(dir.clone());
            }
        }
        dirs
    }

    pub(crate) fn refresh_manifest_legacy_dirs(&self) -> bool {
        let prev: HashSet<PathBuf> = self.manifest_legacy_dirs.lock().values().cloned().collect();
        let next: HashMap<String, PathBuf> = if !self.config.load().auto_detect_project_manifests {
            HashMap::new()
        } else {
            let roots = self.workspace_roots.lock().clone();
            if roots.is_empty() {
                HashMap::new()
            } else {
                let exclude_globs = self.files_exclude.lock().clone();
                crate::project_manifest::discover_manifests(&roots, &exclude_globs)
                    .into_iter()
                    .filter_map(|toml| {
                        let key = path_to_canonical_uri(&toml)?;
                        let root = crate::project_manifest::parse_manifest(&toml)?;
                        Some((key, root))
                    })
                    .collect()
            }
        };
        let next_set: HashSet<PathBuf> = next.values().cloned().collect();
        let changed = prev != next_set;
        tracing::trace!(
            count = next.len(),
            changed,
            "refreshed manifest_legacy_dirs"
        );
        *self.manifest_legacy_dirs.lock() = next;
        changed
    }

    pub(crate) fn apply_manifest_event(
        &self,
        toml_path: &Path,
        typ: lsp_types::FileChangeType,
    ) -> bool {
        let prev: HashSet<PathBuf> = self.manifest_legacy_dirs.lock().values().cloned().collect();
        let resolved = if !self.config.load().auto_detect_project_manifests
            || matches!(typ, lsp_types::FileChangeType::DELETED)
        {
            None
        } else {
            crate::project_manifest::parse_manifest(toml_path)
        };
        let Some(key) = path_to_canonical_uri(toml_path) else {
            return false;
        };
        {
            let mut map = self.manifest_legacy_dirs.lock();
            match resolved {
                Some(root) => {
                    map.insert(key, root);
                }
                None => {
                    map.remove(&key);
                }
            }
        }
        let next: HashSet<PathBuf> = self.manifest_legacy_dirs.lock().values().cloned().collect();
        let changed = prev != next;
        tracing::trace!(
            manifest = %toml_path.display(),
            ?typ,
            changed,
            "applied manifest watcher event"
        );
        changed
    }

    fn uri_under_legacy_dirs(uri: &str, legacy_dirs: &[PathBuf]) -> bool {
        Url::parse(uri)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .is_some_and(|path| legacy_dirs.iter().any(|dir| path.starts_with(dir)))
    }

    // Pairing must see open legacy overrides; those live in workspace_index, not workspace_documents.
    fn legacy_uris_in_workspace_index(&self) -> Vec<String> {
        let legacy_dirs = self.effective_legacy_dirs();
        if legacy_dirs.is_empty() {
            return Vec::new();
        }
        self.workspace_index
            .lock()
            .documents()
            .map(|(uri, _)| uri.to_string())
            .filter(|uri| Self::uri_under_legacy_dirs(uri, &legacy_dirs))
            .collect()
    }

    pub(crate) fn refresh_legacy_override_maps(&self) {
        let base_uris: Vec<String> = self.base_scripts_documents.lock().keys().cloned().collect();
        let legacy_uris = self.legacy_uris_in_workspace_index();
        let (suppressed, replacements) = legacy_base_replacements(&base_uris, &legacy_uris);
        *self.suppressed_base_uris.lock() = suppressed;
        *self.legacy_replacements.lock() = replacements;
        self.rebuild_filtered_base_catalogs();
    }

    pub(crate) fn refresh_legacy_override_maps_if_legacy_uri(&self, uri: &Url) {
        let legacy_dirs = self.effective_legacy_dirs();
        if Self::uri_under_legacy_dirs(uri.as_str(), &legacy_dirs) {
            self.refresh_legacy_override_maps();
        }
    }

    fn remove_legacy_workspace_document(&self, canonical: &str) -> HashSet<String> {
        let mut index = self.workspace_index.lock();
        let mut docs = self.workspace_documents.lock();
        let invalidated = index.remove_document(canonical);
        docs.remove(canonical);
        invalidated
    }

    fn prune_stale_legacy_workspace_files(&self, current: &HashSet<String>) {
        let legacy_dirs = self.effective_legacy_dirs();
        if legacy_dirs.is_empty() {
            return;
        }
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock();
            documents.keys().filter_map(canonical_uri).collect()
        };
        let stale: Vec<String> = self
            .workspace_documents
            .lock()
            .keys()
            .filter(|uri| {
                if current.contains(*uri) || open_canonical.contains(*uri) {
                    return false;
                }
                Self::uri_under_legacy_dirs(uri, &legacy_dirs)
            })
            .cloned()
            .collect();
        if stale.is_empty() {
            return;
        }
        let mut invalidated = HashSet::new();
        for uri in stale {
            invalidated.extend(self.remove_legacy_workspace_document(&uri));
        }
        self.evict_cache_entries(&invalidated);
    }

    fn sync_legacy_workspace_from_parsed(
        &self,
        parsed: Vec<(String, ParsedDocument)>,
    ) -> HashSet<String> {
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock();
            documents.keys().filter_map(canonical_uri).collect()
        };
        let mut current = HashSet::new();
        let mut invalidated = HashSet::new();
        {
            let mut ws_idx = self.workspace_index.lock();
            let mut ws_docs = self.workspace_documents.lock();
            ws_idx.begin_bulk_catalog_update();
            for (uri, document) in parsed {
                current.insert(uri.clone());
                if open_canonical.contains(&uri) {
                    continue;
                }
                invalidated.extend(ws_idx.update_document(uri.as_str(), &document));
                ws_docs.insert(uri, document);
            }
            ws_idx.end_bulk_catalog_update();
        }
        self.prune_stale_legacy_workspace_files(&current);
        invalidated
    }

    fn is_base_script_uri(&self, uri: &Url) -> bool {
        matches!(self.file_scope_of(uri), FileScope::AdditionalBase)
    }

    // index_base_scripts rebuilds the base index from disk, dropping any open base script.
    fn merge_open_base_documents(&self) {
        let open_uris: Vec<Url> = self.documents.lock().keys().cloned().collect();
        let mut base_uris: Vec<Url> = Vec::new();
        for uri in open_uris {
            if self.is_base_script_uri(&uri) {
                base_uris.push(uri);
            }
        }
        if base_uris.is_empty() {
            return;
        }
        let documents = self.documents.lock();
        let mut idx = self.base_scripts_index.lock();
        for uri in base_uris {
            if let Some(doc) = documents.get(&uri) {
                index_open_document(&mut idx, &uri, doc);
            }
        }
    }

    pub(super) fn evict_cache_entries(&self, uris: &HashSet<String>) {
        if uris.is_empty() {
            return;
        }
        let mut cache = self.cst_diag_cache.lock();
        cache.retain(|url, _| !uris.contains(url.as_str()));
    }

    pub(crate) async fn index_workspace(&self) {
        let roots = self.workspace_roots.lock().clone();
        if roots.is_empty() {
            self.workspace_known_files.lock().clear();
            return;
        }
        let exclude_globs = self.files_exclude.lock().clone();

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
            let known_uris: HashSet<String> = files
                .iter()
                .filter_map(|p| Url::from_file_path(p).ok())
                .map(|u| u.to_string())
                .collect();
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
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock();
            documents.keys().filter_map(canonical_uri).collect()
        };

        let mut indexed = 0;
        {
            let mut index = self.workspace_index.lock();
            let mut docs = self.workspace_documents.lock();
            index.begin_bulk_catalog_update();
            for (uri, document) in parsed {
                if open_canonical.contains(&uri) {
                    continue;
                }
                index.update_document(uri.as_str(), &document);
                docs.insert(uri, document);
                indexed += 1;
            }
            index.end_bulk_catalog_update();
        }

        info!(
            indexed,
            file_count,
            elapsed_ms = start.elapsed().as_millis(),
            "workspace indexed"
        );

        self.publish_open_diagnostics();
    }

    pub(crate) fn resolve_at(&self, uri: &Url, position: Position) -> Option<Definition> {
        let documents = self.documents.lock();
        let document = documents.get(uri)?;
        let handles = self.db_handles_for(uri);
        let db = handles.db();
        resolve_definition(uri.as_str(), document, &db, source_position(position))
    }

    pub(crate) async fn index_base_scripts(&self) {
        let game_dir_opt = self.base_scripts_path.lock().clone();
        let extras = self.additional_script_dirs.lock().clone();
        let legacy_dirs = self.effective_legacy_dirs();

        if game_dir_opt.is_none() && extras.is_empty() && legacy_dirs.is_empty() {
            {
                let mut idx = self.base_scripts_index.lock();
                let mut docs = self.base_scripts_documents.lock();
                *idx = WorkspaceIndex::default();
                docs.clear();
            }
            self.legacy_replacements.lock().clear();
            self.suppressed_base_uris.lock().clear();
            self.rebuild_filtered_base_catalogs();
            self.prune_stale_legacy_workspace_files(&HashSet::new());
            self.publish_open_diagnostics();
            self.publish_legacy_script_status();
            self.publish_file_scope_status();
            return;
        }

        if let Some(gd) = &game_dir_opt {
            if let Some(env) = parse_script_environment(&gd.join(r"bin\redscripts.ini")) {
                *self.script_env.lock() = env;
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
                info!(
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

        {
            let mut idx = self.base_scripts_index.lock();
            let mut docs = self.base_scripts_documents.lock();
            *idx = base_new_index;
            *docs = base_new_docs;
        }
        *self.legacy_replacements.lock() = legacy_replacements;
        *self.suppressed_base_uris.lock() = suppressed_base;
        self.merge_open_base_documents();
        self.rebuild_filtered_base_catalogs();

        let invalidated = self.sync_legacy_workspace_from_parsed(legacy_parsed);
        self.evict_cache_entries(&invalidated);

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

        self.publish_open_diagnostics();
        self.publish_legacy_script_status();
        self.publish_file_scope_status();
    }
}
