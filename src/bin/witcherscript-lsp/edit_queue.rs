use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use lsp_types::Url;
use tracing::{debug, error, trace};
use tree_sitter::{Parser, Tree};
use witcherscript_language::document::{
    allocate_parse_version, parse_document_with_prior, ParsedDocument,
};
use witcherscript_language::line_index::LineIndex;

use crate::backend::Backend;

#[derive(Debug, Clone)]
pub(crate) struct PendingEdit {
    pub source: String,
    pub line_index: LineIndex,
    pub tree: Tree,
    pub target_parse_version: u64,
}

impl Backend {
    // Returns the latest known (source, line_index, tree) for `uri`: from pending if an edit is
    // queued but not yet processed, otherwise from the published Compilation. None if neither has it.
    pub(crate) fn latest_edit_state(&self, uri: &Url) -> Option<(String, LineIndex, Tree)> {
        if let Some(pending) = self.pending_edits.lock().get(uri) {
            return Some((
                pending.source.clone(),
                pending.line_index.clone(),
                pending.tree.clone(),
            ));
        }
        let snap = self.snapshot();
        snap.documents
            .get(uri)
            .map(|doc| (doc.source.clone(), doc.line_index.clone(), doc.tree.clone()))
    }

    pub(crate) fn enqueue_edit(&self, uri: Url, source: String, line_index: LineIndex, tree: Tree) {
        let target_parse_version = allocate_parse_version();
        let edit = PendingEdit {
            source,
            line_index,
            tree,
            target_parse_version,
        };
        if !self.edit_writer_spawned.load(Ordering::Acquire) {
            trace!(op = "enqueue_edit", uri = %uri, path = "sync", target_parse_version, "enter");
            self.process_pending_edit(uri, edit);
            self.notify_diagnostics_changed();
            return;
        }
        self.pending_edits.lock().insert(uri.clone(), edit);
        let version = self.diagnostic_version.fetch_add(1, Ordering::AcqRel) + 1;
        trace!(op = "enqueue_edit", uri = %uri, path = "async", version, target_parse_version, "queued");
        self.edit_notify.notify_one();
    }

    pub(crate) fn pending_target_for(&self, uri: &Url) -> Option<u64> {
        self.pending_edits
            .lock()
            .get(uri)
            .map(|e| e.target_parse_version)
    }

    pub(crate) fn spawn_edit_writer(&self) {
        if self.edit_writer_spawned.swap(true, Ordering::AcqRel) {
            return;
        }
        let backend = self.clone();
        tokio::spawn(async move {
            backend.run_edit_writer().await;
        });
    }

    async fn run_edit_writer(&self) {
        loop {
            self.edit_notify.notified().await;
            loop {
                let uris: Vec<Url> = {
                    let pending = self.pending_edits.lock();
                    pending.keys().cloned().collect()
                };
                if uris.is_empty() {
                    break;
                }
                let backend = self.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    for uri in uris {
                        // Keep the entry until publish so a racing did_change sees it via latest_edit_state.
                        let Some(edit) = backend.clone_pending_for(&uri) else {
                            continue;
                        };
                        let processed_target = edit.target_parse_version;
                        backend.process_pending_edit(uri.clone(), edit);
                        let mut pending = backend.pending_edits.lock();
                        if let Some(current) = pending.get(&uri) {
                            if current.target_parse_version == processed_target {
                                pending.remove(&uri);
                            }
                        }
                    }
                })
                .await;
                self.request_workspace_diagnostic_refresh();
            }
        }
    }

    pub(crate) fn clone_pending_for(&self, uri: &Url) -> Option<PendingEdit> {
        self.pending_edits.lock().get(uri).cloned()
    }

    pub(crate) fn process_pending_edit(&self, uri: Url, edit: PendingEdit) {
        let started_at = Instant::now();
        let bytes = edit.source.len();
        trace!(op = "process_pending_edit", uri = %uri, bytes, "start");
        let parse_at = Instant::now();
        let mut parser = Parser::new();
        if let Err(err) = parser.set_language(&tree_sitter_witcherscript::language()) {
            error!(uri = %uri, error = %err, "failed to load WitcherScript grammar");
            return;
        }
        let parsed = parse_document_with_prior(&mut parser, edit.source, Some(&edit.tree));
        let parse_us = parse_at.elapsed().as_micros();
        let mut document = match parsed {
            Ok(document) => document,
            Err(err) => {
                error!(uri = %uri, error = %err, "failed to parse document");
                return;
            }
        };
        document.parse_version = edit.target_parse_version;
        let parse_version = document.parse_version;
        let document: Arc<ParsedDocument> = Arc::new(document);

        let publish_at = Instant::now();
        trace!(op = "process_pending_edit", uri = %uri, parse_version, "publishing");
        self.publish_open_document_indices(&uri, &document);
        let publish_us = publish_at.elapsed().as_micros();
        trace!(op = "process_pending_edit", uri = %uri, parse_version, "published");

        debug!(
            op = "process_pending_edit",
            uri = %uri,
            bytes,
            parse_us,
            publish_us,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }
}
