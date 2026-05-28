use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use lsp_types::{Position, Url};
use tracing::{debug, error, trace, warn};
use tree_sitter::{Parser, Tree};
use witcherscript_language::document::{parse_document, parse_document_with_prior};
use witcherscript_language::files::{canonical_uri, read_script_file};
use witcherscript_language::resolve::{resolve_definition, Definition, ObservedKey};

use crate::backend::Backend;
use crate::compilation::CompilationBuilder;
use crate::convert::source_position;
use crate::file_scope::{classify_file_scope, FileScope};

use super::helpers::{reindex_into, remove_document_all_spellings};

fn route_document_to_index(
    builder: &mut CompilationBuilder,
    uri: &Url,
    scope: &FileScope,
    document: &witcherscript_language::document::ParsedDocument,
) -> (Vec<ObservedKey>, Vec<ObservedKey>) {
    match scope {
        FileScope::AdditionalBase => {
            let _ = builder
                .base_scripts_index_mut()
                .update_document(uri.as_str(), document);
            (Vec::new(), Vec::new())
        }
        FileScope::OutOfScope | FileScope::SingleFile => {
            let loose = builder
                .loose_index_mut()
                .update_document(uri.as_str(), document);
            (Vec::new(), loose)
        }
        _ => {
            let ws = builder
                .workspace_index_mut()
                .update_document(uri.as_str(), document);
            (ws, Vec::new())
        }
    }
}

impl Backend {
    // A workspace-folder or config change reroutes open docs now, not on next keystroke.
    pub(crate) fn reindex_open_documents(&self) {
        let snap = self.snapshot();
        if snap.documents.is_empty() {
            return;
        }
        let started_at = Instant::now();
        let docs = snap.documents.len();
        debug!(op = "reindex_open_documents", docs, "start",);
        let roots = self.workspace_roots.lock().clone();
        let legacy_dirs = self.effective_legacy_dirs();
        let game_dir = self.base_scripts_path.lock().clone();
        let additional = self.additional_script_dirs.lock().clone();
        let replacements = self.legacy_replacements.lock().clone();
        let scopes: Vec<(Url, FileScope)> = snap
            .documents
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

        let mut ws_changed: Vec<ObservedKey> = Vec::new();
        let mut loose_changed: Vec<ObservedKey> = Vec::new();
        let mut invalidated: HashSet<String> = HashSet::new();
        self.publish_compilation(|builder| {
            let docs = builder.base.documents.clone();
            let workspace = builder.workspace_index_mut();
            for (uri, _) in &scopes {
                ws_changed.extend(remove_document_all_spellings(workspace, uri));
            }
            let loose = builder.loose_index_mut();
            for (uri, _) in &scopes {
                loose_changed.extend(remove_document_all_spellings(loose, uri));
            }
            let base = builder.base_scripts_index_mut();
            for (uri, _) in &scopes {
                // Base scripts have no editor subscribers, so the returned keys go unused.
                let _ = remove_document_all_spellings(base, uri);
            }
            for (uri, scope) in &scopes {
                let Some(document) = docs.get(uri) else {
                    continue;
                };
                let (ws, loose) = route_document_to_index(builder, uri, scope, document.as_ref());
                ws_changed.extend(ws);
                loose_changed.extend(loose);
                invalidated.insert(uri.to_string());
            }
        });
        invalidated.extend(self.invalidated_workspace(&ws_changed));
        invalidated.extend(self.invalidated_loose(&loose_changed));
        self.evict_cache_entries(&invalidated);
        debug!(
            op = "reindex_open_documents",
            docs,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    // Closing a non-loose file reverts it from buffer to on-disk content in the workspace/base index.
    pub(crate) fn reindex_closed_file(&self, uri: &Url) {
        let started_at = Instant::now();
        debug!(
            op = "reindex_closed_file",
            uri = %uri,
            "start",
        );
        let canonical = canonical_uri(uri).unwrap_or_else(|| uri.to_string());
        let is_base = self.is_base_script_uri(uri);
        let parsed = match uri.to_file_path() {
            Ok(path) => match read_script_file(&path) {
                Ok(text) => parse_document(text)
                    .map_err(|e| warn!(uri = %uri, error = %e, "failed to parse closed file"))
                    .ok(),
                Err(e) => {
                    warn!(uri = %uri, error = %e, "failed to read closed file");
                    None
                }
            },
            Err(_) => {
                warn!(uri = %uri, "closed file URI is not a file path");
                None
            }
        };

        let mut changed: Vec<ObservedKey> = Vec::new();
        self.publish_compilation(|builder| {
            if is_base {
                let (index, docs) = builder.base_scripts_index_and_docs_mut();
                let _ = reindex_into(index, docs, uri.as_str(), &canonical, parsed);
            } else {
                let (index, docs) = builder.workspace_index_and_docs_mut();
                changed.extend(reindex_into(index, docs, uri.as_str(), &canonical, parsed));
            }
        });
        let invalidated = if is_base {
            HashSet::new()
        } else {
            self.invalidated_workspace(&changed)
        };
        self.evict_cache_entries(&invalidated);
        debug!(
            op = "reindex_closed_file",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    pub(crate) fn update_open_document(&self, uri: Url, text: String) {
        self.update_open_document_with_prior(uri, text, None);
    }

    pub(crate) fn update_open_document_with_prior(
        &self,
        uri: Url,
        text: String,
        prior_tree: Option<Tree>,
    ) {
        let started_at = Instant::now();
        let bytes = text.len();
        let had_prior_tree = prior_tree.is_some();
        trace!(
            op = "update_open_document",
            uri = %uri,
            bytes,
            "start",
        );
        let parse_at = Instant::now();
        let parsed = match prior_tree {
            Some(tree) => {
                let mut parser = Parser::new();
                match parser.set_language(&tree_sitter_witcherscript::language()) {
                    Ok(()) => parse_document_with_prior(&mut parser, text, Some(&tree)),
                    Err(err) => {
                        error!(uri = %uri, error = %err, "failed to load WitcherScript grammar");
                        return;
                    }
                }
            }
            None => parse_document(text),
        };
        let parse_us = parse_at.elapsed().as_micros();
        let document = match parsed {
            Ok(document) => document,
            Err(err) => {
                error!(uri = %uri, error = %err, "failed to parse document");
                return;
            }
        };
        let document = Arc::new(document);

        let publish_at = Instant::now();
        self.publish_open_document_indices(&uri, &document);
        let publish_us = publish_at.elapsed().as_micros();
        self.notify_diagnostics_changed();
        let version = self
            .diagnostic_version
            .load(std::sync::atomic::Ordering::Acquire);
        trace!(
            op = "update_open_document",
            uri = %uri,
            bytes,
            version,
            had_prior_tree,
            parse_us,
            publish_us,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    pub(crate) fn publish_open_document_indices(
        &self,
        uri: &Url,
        document: &Arc<witcherscript_language::document::ParsedDocument>,
    ) {
        let scope = self.file_scope_of(uri);
        let mut ws_changed: Vec<ObservedKey> = Vec::new();
        let mut loose_changed: Vec<ObservedKey> = Vec::new();
        self.publish_compilation(|builder| {
            ws_changed.extend(remove_document_all_spellings(
                builder.workspace_index_mut(),
                uri,
            ));
            // Base scripts have no editor subscribers, so the returned keys go unused.
            let _ = remove_document_all_spellings(builder.base_scripts_index_mut(), uri);
            loose_changed.extend(remove_document_all_spellings(
                builder.loose_index_mut(),
                uri,
            ));

            let (ws, loose) = route_document_to_index(builder, uri, &scope, document.as_ref());
            ws_changed.extend(ws);
            loose_changed.extend(loose);
            builder
                .documents_mut()
                .insert(uri.clone(), document.clone());
        });
        let mut invalidated = self.invalidated_workspace(&ws_changed);
        invalidated.extend(self.invalidated_loose(&loose_changed));
        self.evict_cache_entries(&invalidated);
    }

    pub(crate) fn evict_cache_entries(&self, uris: &HashSet<String>) {
        if uris.is_empty() {
            return;
        }
        let mut cache = self.cst_diag_cache.lock();
        cache.retain(|url, _| !uris.contains(url.as_str()));
    }

    pub(crate) fn resolve_at(&self, uri: &Url, position: Position) -> Option<Definition> {
        let snap = self.snapshot();
        let document = snap.documents.get(uri)?.clone();
        let handles = self.db_handles_for_with_snapshot(uri, &snap);
        let db = handles.db();
        resolve_definition(uri.as_str(), &document, &db, source_position(position))
    }
}
