use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lsp_types::Url;
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::files::canonical_uri;

use crate::backend::Backend;

use super::helpers::{legacy_base_replacements, mod_shared_imports_dir, path_to_canonical_uri};

impl Backend {
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

    fn remove_legacy_workspace_document(
        &self,
        canonical: &str,
    ) -> Vec<witcherscript_language::resolve::ObservedKey> {
        let mut index = self.workspace_index.lock();
        let mut docs = self.workspace_documents.lock();
        let changed = index.remove_document(canonical);
        docs.remove(canonical);
        changed
    }

    pub(crate) fn prune_stale_legacy_workspace_files(&self, current: &HashSet<String>) {
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
        let mut ws_changed: Vec<witcherscript_language::resolve::ObservedKey> = Vec::new();
        for uri in stale {
            ws_changed.extend(self.remove_legacy_workspace_document(&uri));
        }
        let invalidated = self.invalidated_workspace(&ws_changed);
        self.evict_cache_entries(&invalidated);
    }

    pub(crate) fn sync_legacy_workspace_from_parsed(
        &self,
        parsed: Vec<(String, ParsedDocument)>,
    ) -> HashSet<String> {
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock();
            documents.keys().filter_map(canonical_uri).collect()
        };
        let mut current = HashSet::new();
        let mut ws_changed: Vec<witcherscript_language::resolve::ObservedKey> = Vec::new();
        {
            let mut ws_idx = self.workspace_index.lock();
            let mut ws_docs = self.workspace_documents.lock();
            ws_idx.begin_bulk_catalog_update();
            for (uri, document) in parsed {
                current.insert(uri.clone());
                if open_canonical.contains(&uri) {
                    continue;
                }
                ws_changed.extend(ws_idx.update_document(uri.as_str(), &document));
                ws_docs.insert(uri, document);
            }
            ws_idx.end_bulk_catalog_update();
        }
        let invalidated = self.invalidated_workspace(&ws_changed);
        self.prune_stale_legacy_workspace_files(&current);
        invalidated
    }
}
