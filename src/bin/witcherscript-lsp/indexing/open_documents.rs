use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use lsp_types::{Position, Url};
use tracing::{debug, error, trace, warn};
use tree_sitter::{Parser, Tree};
use witcherscript_language::document::{parse_document, parse_document_with_prior};
use witcherscript_language::files::{canonical_uri, read_text_file};
use witcherscript_language::resolve::{resolve_definition, Definition, ObservedKey};

use crate::backend::Backend;
use crate::compilation::CompilationBuilder;
use crate::convert::source_position;
use crate::file_scope::{classify_file_scope, FileScope};

use super::helpers::{index_open_document, reindex_into, remove_document_all_spellings};

// index_open_document updates the target in place, keeping its diff baseline so an unchanged reopen evicts nothing.
fn route_document_to_index(
    builder: &mut CompilationBuilder,
    uri: &Url,
    scope: &FileScope,
    document: &witcherscript_language::document::ParsedDocument,
) -> (Vec<ObservedKey>, Vec<ObservedKey>) {
    // Only workspace and loose changes feed invalidation (there is no invalidated_base), so base-script keys are dropped.
    match scope {
        FileScope::AdditionalBase => {
            let ws = remove_document_all_spellings(builder.workspace_index_mut(), uri);
            let loose = remove_document_all_spellings(builder.loose_index_mut(), uri);
            let _ = index_open_document(builder.base_scripts_index_mut(), uri, document);
            (ws, loose)
        }
        FileScope::OutOfScope | FileScope::SingleFile => {
            let ws = remove_document_all_spellings(builder.workspace_index_mut(), uri);
            let _ = remove_document_all_spellings(builder.base_scripts_index_mut(), uri);
            let loose = index_open_document(builder.loose_index_mut(), uri, document);
            (ws, loose)
        }
        _ => {
            let _ = remove_document_all_spellings(builder.base_scripts_index_mut(), uri);
            let loose = remove_document_all_spellings(builder.loose_index_mut(), uri);
            let ws = index_open_document(builder.workspace_index_mut(), uri, document);
            (ws, loose)
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
        let base_scripts_dir = self.base_scripts_dir();
        let additional = self.config.load().additional_script_dirs.clone();
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
                        base_scripts_dir.as_deref(),
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
            for (uri, scope) in &scopes {
                let Some(document) = docs.get(uri) else {
                    continue;
                };
                let (ws, loose) = route_document_to_index(builder, uri, scope, document.as_ref());
                ws_changed.extend(ws);
                loose_changed.extend(loose);
                invalidated.insert(canonical_uri(uri));
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

    // When an edited buffer is closed, any unsaved changes need to be reverted to the on-disk content.
    pub(crate) fn reindex_closed_file(&self, uri: &Url, prior_source: Option<&str>) -> bool {
        let started_at = Instant::now();
        debug!(
            op = "reindex_closed_file",
            uri = %uri,
            "start",
        );
        let canonical = canonical_uri(uri);
        let is_base = self.is_base_script_uri(uri);
        let disk_text = match uri.to_file_path() {
            Ok(path) => match read_text_file(&path) {
                Ok(text) => Some(text),
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

        if let (Some(prior), Some(disk)) = (prior_source, disk_text.as_deref()) {
            if prior == disk {
                debug!(op = "reindex_closed_file", uri = %uri, "unedited buffer; skipped reindex");
                return false;
            }
        }

        let parsed = match disk_text {
            Some(text) => parse_document(text)
                .map_err(|e| warn!(uri = %uri, error = %e, "failed to parse closed file"))
                .ok(),
            None => None,
        };

        let mut changed: Vec<ObservedKey> = Vec::new();
        self.publish_compilation(|builder| {
            if is_base {
                // Base-script changes feed no invalidation (see the is_base guard below), so drop the changed keys.
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
        true
    }

    pub(crate) fn update_open_document(&self, uri: Url, text: String) -> bool {
        self.update_open_document_with_prior(uri, text, None)
    }

    // A new parse_version invalidates the diagnostic result_id and forces a redundant
    // workspace recompute; reuse the parse we already hold when the bytes are unchanged.
    fn reuse_unchanged_open_document(&self, uri: &Url, text: &str) -> bool {
        let snap = self.snapshot();
        let existing = snap.documents.get(uri).cloned().or_else(|| {
            let canonical = canonical_uri(uri);
            snap.workspace_documents
                .get(&canonical)
                .or_else(|| snap.base_scripts_documents.get(&canonical))
                .cloned()
        });
        let Some(existing) = existing else {
            // Loose files are legitimately unindexed; only an in-scope miss is a real error.
            if !self.file_scope_of(uri).is_loose() {
                trace!(uri = %uri, "no indexed copy for in-scope file on open; canonical_uri did not match the index key");
            }
            return false;
        };
        if existing.source != text {
            trace!(uri = %uri, "open bytes differ from indexed copy; forcing reparse");
            return false;
        }
        // Edit tracking and document-pull read the open overlay, so it must hold the file.
        if !snap.documents.contains_key(uri) {
            let uri = uri.clone();
            self.publish_compilation(move |builder| {
                builder.documents_mut().insert(uri, existing);
            });
        }
        true
    }

    pub(crate) fn update_open_document_with_prior(
        &self,
        uri: Url,
        text: String,
        prior_tree: Option<Tree>,
    ) -> bool {
        if self.reuse_unchanged_open_document(&uri, &text) {
            trace!(op = "update_open_document", uri = %uri, "bytes unchanged; reused parse");
            return false;
        }
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
                        return false;
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
                return false;
            }
        };
        let document = Arc::new(document);

        let publish_at = Instant::now();
        self.publish_open_document_indices(&uri, &document);
        let publish_us = publish_at.elapsed().as_micros();
        trace!(
            op = "update_open_document",
            uri = %uri,
            bytes,
            had_prior_tree,
            parse_us,
            publish_us,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        true
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
        resolve_definition(
            &canonical_uri(uri),
            &document,
            &db,
            source_position(position),
        )
    }
}
