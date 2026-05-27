use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use lsp_types::Url;
use tracing::{debug, error};
use tree_sitter::{Parser, Tree};
use witcherscript_language::document::{parse_document_with_prior, ParsedDocument};
use witcherscript_language::line_index::LineIndex;

use crate::backend::Backend;
use crate::logging::wall_clock_us;

#[derive(Debug)]
pub(crate) struct PendingEdit {
    pub source: String,
    pub line_index: LineIndex,
    pub tree: Tree,
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

    pub(crate) fn enqueue_edit(&self, uri: Url, edit: PendingEdit) {
        if !self.edit_writer_spawned.load(Ordering::Acquire) {
            self.process_pending_edit(uri, edit);
            self.spawn_diagnostics_state_changed();
            return;
        }
        self.pending_edits.lock().insert(uri, edit);
        self.diagnostic_version.fetch_add(1, Ordering::AcqRel);
        self.edit_notify.notify_one();
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
                let batch: Vec<(Url, PendingEdit)> = {
                    let mut pending = self.pending_edits.lock();
                    pending.drain().collect()
                };
                if batch.is_empty() {
                    break;
                }
                let backend = self.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    for (uri, edit) in batch {
                        backend.process_pending_edit(uri, edit);
                    }
                })
                .await;
                self.spawn_diagnostics_at_current_version();
            }
        }
    }

    pub(crate) fn process_pending_edit(&self, uri: Url, edit: PendingEdit) {
        let started_at = Instant::now();
        let bytes = edit.source.len();
        let parse_at = Instant::now();
        let mut parser = Parser::new();
        if let Err(err) = parser.set_language(&tree_sitter_witcherscript::language()) {
            error!(uri = %uri, error = %err, "failed to load WitcherScript grammar");
            return;
        }
        let parsed = parse_document_with_prior(&mut parser, edit.source, Some(&edit.tree));
        let parse_us = parse_at.elapsed().as_micros();
        let document = match parsed {
            Ok(document) => document,
            Err(err) => {
                error!(uri = %uri, error = %err, "failed to parse document");
                return;
            }
        };
        let document: Arc<ParsedDocument> = Arc::new(document);

        let publish_at = Instant::now();
        self.publish_open_document_indices(&uri, &document);
        let publish_us = publish_at.elapsed().as_micros();

        debug!(
            op = "process_pending_edit",
            uri = %uri,
            bytes,
            parse_us,
            publish_us,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
            "complete",
        );
    }
}
