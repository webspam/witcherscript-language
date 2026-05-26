use std::collections::HashSet;

use lsp_types::{Position, Url};
use tracing::error;
use witcherscript_language::document::parse_document;
use witcherscript_language::files::{canonical_uri, read_script_file};
use witcherscript_language::resolve::{resolve_definition, Definition};

use crate::backend::Backend;
use crate::convert::source_position;
use crate::file_scope::{classify_file_scope, FileScope};

use super::helpers::{reindex_into, remove_document_all_spellings};

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
                let target: &mut witcherscript_language::resolve::WorkspaceIndex = match scope {
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

    pub(crate) fn evict_cache_entries(&self, uris: &HashSet<String>) {
        if uris.is_empty() {
            return;
        }
        let mut cache = self.cst_diag_cache.lock();
        cache.retain(|url, _| !uris.contains(url.as_str()));
    }

    pub(crate) fn resolve_at(&self, uri: &Url, position: Position) -> Option<Definition> {
        let documents = self.documents.lock();
        let document = documents.get(uri)?;
        let handles = self.db_handles_for(uri);
        let db = handles.db();
        resolve_definition(uri.as_str(), document, &db, source_position(position))
    }
}
