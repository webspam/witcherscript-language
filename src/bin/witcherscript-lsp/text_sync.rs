use std::path::PathBuf;
use std::time::Instant;

use lsp_types::{
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, Url,
};
use tracing::{error, trace};
use witcherscript_language::builtins::builtin_source;
use witcherscript_language::document::apply_content_change;
use witcherscript_language::line_index::LineIndex;

use crate::backend::Backend;
use crate::convert::{source_position, source_range};

fn uri_within_any(uri: &str, dirs: &[PathBuf]) -> bool {
    let Some(path) = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()) else {
        return false;
    };
    dirs.iter().any(|dir| path.starts_with(dir))
}

impl Backend {
    pub(crate) fn _did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        if self.is_uri_excluded(&uri) {
            return;
        }
        let started_at = Instant::now();
        trace!(op = "did_open", uri = %uri, "start");
        // The client drops a file's status on close; force a fresh push.
        self.sent_legacy_status.lock().remove(&uri);
        self.sent_file_scope_status.lock().remove(&uri);
        let legacy_dirs = self.effective_legacy_dirs();
        let reindexed = self.update_open_document(uri.clone(), params.text_document.text);
        // A reused (byte-identical) open changes no override map, and already notified internally.
        if reindexed && uri_within_any(uri.as_str(), &legacy_dirs) {
            self.refresh_legacy_override_maps();
        }
        self.publish_legacy_script_status();
        self.publish_file_scope_status();
        trace!(
            op = "did_open",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    pub(crate) fn _did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        if self.is_uri_excluded(&uri) {
            return;
        }
        let started_at = Instant::now();
        trace!(
            op = "did_change",
            uri = %uri,
            version = params.text_document.version,
            changes = params.content_changes.len(),
            "start",
        );

        let _tree_guard = self.tree_pipeline.lock();
        let Some((mut source, mut line_index, mut prior_tree)) = self.latest_edit_state(&uri)
        else {
            // VS Code replays did_change for restored dirty editors before did_open.
            trace!(uri = %uri, "did_change before did_open; ignoring");
            return;
        };

        let mut prior_tree_valid = true;
        for change in params.content_changes {
            let text_preview: String = change.text.chars().take(64).collect();
            trace!(
                op = "did_change",
                uri = %uri,
                range = ?change.range,
                text_len = change.text.len(),
                text = ?text_preview,
                "raw content change",
            );
            let range = change
                .range
                .map(|r| source_range(source_position(r.start), source_position(r.end)));
            if let Some((next, edit)) =
                apply_content_change(&source, &line_index, range, &change.text)
            {
                match edit {
                    Some(edit) if prior_tree_valid => prior_tree.edit(&edit),
                    // A None edit means full-document replace; drop the prior tree.
                    None => prior_tree_valid = false,
                    _ => {}
                }
                line_index = LineIndex::new(&next);
                source = next;
            } else {
                error!(uri = %uri, "out-of-range incremental change; dropping batch");
                return;
            }
        }

        if prior_tree_valid {
            self.enqueue_edit(uri.clone(), source, line_index, prior_tree);
        } else {
            // Full-document replace: prior tree is invalid; bypass the queue and re-parse from scratch.
            self.update_open_document(uri.clone(), source);
        }
        trace!(
            op = "did_change",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    pub(crate) fn _did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        let started_at = Instant::now();
        trace!(op = "did_close", uri = %uri, "start");
        let scope = self.file_scope_of(&uri);
        // reindex_closed_file would re-add an excluded (gitignored / files.exclude) file from disk; drop it instead.
        let excluded = !scope.is_loose() && self.is_uri_excluded(&uri);
        let prior_source = if scope.is_loose() || excluded {
            None
        } else {
            self.snapshot()
                .documents
                .get(&uri)
                .map(|doc| doc.source.clone())
        };
        let mut loose_changed: Vec<witcherscript_language::resolve::ObservedKey> = Vec::new();
        let mut ws_changed: Vec<witcherscript_language::resolve::ObservedKey> = Vec::new();
        self.publish_compilation(|builder| {
            builder.documents_mut().remove(&uri);
            if scope.is_loose() {
                loose_changed.extend(crate::indexing::remove_document_all_spellings(
                    builder.loose_index_mut(),
                    &uri,
                ));
            } else if excluded {
                ws_changed.extend(crate::indexing::remove_document_all_spellings(
                    builder.workspace_index_mut(),
                    &uri,
                ));
            }
        });
        if scope.is_loose() {
            // A loose file is a transient compilation member: closing it drops it from the index entirely.
            let invalidated = self.invalidated_loose(&loose_changed);
            self.evict_cache_entries(&invalidated);
        } else if excluded {
            let invalidated = self.invalidated_workspace(&ws_changed);
            self.evict_cache_entries(&invalidated);
        } else if self.reindex_closed_file(&uri, prior_source.as_deref()) {
            self.refresh_legacy_override_maps_if_legacy_uri(&uri);
        }
        self.publish_file_scope_status();
        self.sent_file_scope_status.lock().remove(&uri);
        self.semantic_tokens_cache.lock().remove(&uri);
        trace!(
            op = "did_close",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    pub(crate) fn _did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let started_at = Instant::now();
        let count = params.changes.len();
        trace!(op = "did_change_watched_files", count, "start",);
        self.apply_watched_file_events(params.changes);
        trace!(
            op = "did_change_watched_files",
            count,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }

    pub(crate) async fn _did_change_workspace_folders(
        &self,
        params: DidChangeWorkspaceFoldersParams,
    ) {
        let started_at = Instant::now();
        trace!(
            op = "did_change_workspace_folders",
            added = params.event.added.len(),
            removed = params.event.removed.len(),
            "start",
        );
        let removed: Vec<PathBuf> = params
            .event
            .removed
            .iter()
            .filter_map(|folder| folder.uri.to_file_path().ok())
            .collect();
        let added: Vec<PathBuf> = params
            .event
            .added
            .iter()
            .filter_map(|folder| folder.uri.to_file_path().ok())
            .collect();

        self.workspace_roots.rcu(|roots| {
            let mut next = (**roots).clone();
            next.retain(|root| !removed.iter().any(|dir| root.starts_with(dir)));
            for path in &added {
                if !next.contains(path) {
                    next.push(path.clone());
                }
            }
            next
        });

        // index_workspace only adds files; a removed folder's scripts must be dropped here.
        if !removed.is_empty() {
            let mut ws_changed: Vec<witcherscript_language::resolve::ObservedKey> = Vec::new();
            self.publish_compilation(|builder| {
                let stale: Vec<String> = builder
                    .base
                    .workspace_documents
                    .keys()
                    .filter(|uri| uri_within_any(uri, &removed))
                    .cloned()
                    .collect();
                let docs = builder.workspace_documents_mut();
                for uri in &stale {
                    docs.remove(uri);
                }
                let index = builder.workspace_index_mut();
                for uri in &stale {
                    ws_changed.extend(index.remove_document(uri));
                }
            });
            let invalidated = self.invalidated_workspace(&ws_changed);
            self.evict_cache_entries(&invalidated);
        }

        self.index_workspace().await;
        if self.refresh_manifest_legacy_dirs() {
            self.index_base_scripts().await;
        }
        self.reindex_open_documents();
        self.publish_file_scope_status();
        trace!(
            op = "did_change_workspace_folders",
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
    }
}
