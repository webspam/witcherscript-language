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
use crate::logging::wall_clock_us;

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
        trace!(op = "did_open", uri = %uri, at = %wall_clock_us(), "start");
        // The client drops a file's status on close; force a fresh push.
        self.sent_legacy_status.lock().remove(&uri);
        self.sent_file_scope_status.lock().remove(&uri);
        let legacy_dirs = self.effective_legacy_dirs();
        self.update_open_document(uri.clone(), params.text_document.text);
        if uri_within_any(uri.as_str(), &legacy_dirs) {
            self.refresh_legacy_override_maps();
            self.spawn_diagnostics_state_changed();
        }
        self.publish_legacy_script_status();
        self.publish_file_scope_status();
        trace!(
            op = "did_open",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
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
        trace!(op = "did_change", uri = %uri, at = %wall_clock_us(), "start");
        let prior = self
            .snapshot()
            .documents
            .get(&uri)
            .map(|d| (d.source.clone(), d.line_index.clone(), d.tree.clone()));

        let Some((mut source, mut line_index, mut prior_tree)) = prior else {
            error!(uri = %uri, "did_change before did_open");
            return;
        };

        let mut prior_tree_valid = true;
        for change in params.content_changes {
            let range = change
                .range
                .map(|r| source_range(source_position(r.start), source_position(r.end)));
            match apply_content_change(&source, &line_index, range, &change.text) {
                Some((next, edit)) => {
                    match edit {
                        Some(edit) if prior_tree_valid => prior_tree.edit(&edit),
                        // A None edit means full-document replace; drop the prior tree.
                        None => prior_tree_valid = false,
                        _ => {}
                    }
                    line_index = LineIndex::new(&next);
                    source = next;
                }
                None => {
                    error!(uri = %uri, "out-of-range incremental change; dropping batch");
                    return;
                }
            }
        }

        let prior = if prior_tree_valid {
            Some(prior_tree)
        } else {
            None
        };
        self.update_open_document_with_prior(uri.clone(), source, prior);
        trace!(
            op = "did_change",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
            "complete",
        );
    }

    pub(crate) fn _did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        let started_at = Instant::now();
        trace!(op = "did_close", uri = %uri, at = %wall_clock_us(), "start");
        let scope = self.file_scope_of(&uri);
        let mut loose_changed: Vec<witcherscript_language::resolve::ObservedKey> = Vec::new();
        self.publish_compilation(|builder| {
            builder.documents_mut().remove(&uri);
            if scope.is_loose() {
                loose_changed.extend(crate::indexing::remove_document_all_spellings(
                    builder.loose_index_mut(),
                    &uri,
                ));
            }
        });
        if scope.is_loose() {
            // A loose file is a transient compilation member: closing it drops it from the index entirely.
            let invalidated = self.invalidated_loose(&loose_changed);
            self.evict_cache_entries(&invalidated);
        } else {
            self.reindex_closed_file(&uri);
            self.refresh_legacy_override_maps_if_legacy_uri(&uri);
        }
        self.spawn_diagnostics_state_changed();
        self.publish_file_scope_status();
        self.sent_file_scope_status.lock().remove(&uri);
        trace!(
            op = "did_close",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
            "complete",
        );
    }

    pub(crate) fn _did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let started_at = Instant::now();
        let count = params.changes.len();
        trace!(
            op = "did_change_watched_files",
            count,
            at = %wall_clock_us(),
            "start",
        );
        self.apply_watched_file_events(params.changes);
        trace!(
            op = "did_change_watched_files",
            count,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
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
            at = %wall_clock_us(),
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

        {
            let mut roots = self.workspace_roots.lock();
            roots.retain(|root| !removed.iter().any(|dir| root.starts_with(dir)));
            for path in &added {
                if !roots.contains(path) {
                    roots.push(path.clone());
                }
            }
        }

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
        self.diagnostics_state_changed();
        self.publish_file_scope_status();
        trace!(
            op = "did_change_workspace_folders",
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
            "complete",
        );
    }
}
