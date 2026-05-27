use std::path::PathBuf;

use lsp_types::{
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, Url,
};
use tracing::error;
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
    }

    #[tracing::instrument(skip_all, fields(uri = %params.text_document.uri), level = "debug")]
    pub(crate) fn _did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        if self.is_uri_excluded(&uri) {
            return;
        }
        let prior = self
            .documents
            .lock()
            .get(&uri)
            .map(|d| (d.source.clone(), d.line_index.clone()));

        let Some((mut source, mut line_index)) = prior else {
            error!(uri = %uri, "did_change before did_open");
            return;
        };

        for change in params.content_changes {
            let range = change
                .range
                .map(|r| source_range(source_position(r.start), source_position(r.end)));
            match apply_content_change(&source, &line_index, range, &change.text) {
                Some(next) => {
                    line_index = LineIndex::new(&next);
                    source = next;
                }
                None => {
                    error!(uri = %uri, "out-of-range incremental change; dropping batch");
                    return;
                }
            }
        }

        self.update_open_document(uri, source);
    }

    pub(crate) fn _did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return;
        }
        let scope = self.file_scope_of(&uri);
        self.documents.lock().remove(&uri);
        if scope.is_loose() {
            // A loose file is a transient compilation member: closing it drops it from the index entirely.
            let changed = {
                let mut index = self.loose_index.lock();
                crate::indexing::remove_document_all_spellings(&mut index, &uri)
            };
            let invalidated = self.invalidated_loose(&changed);
            self.evict_cache_entries(&invalidated);
        } else {
            self.reindex_closed_file(&uri);
            self.refresh_legacy_override_maps_if_legacy_uri(&uri);
        }
        self.spawn_diagnostics_state_changed();
        self.publish_file_scope_status();
        self.sent_file_scope_status.lock().remove(&uri);
    }

    pub(crate) fn _did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.apply_watched_file_events(params.changes);
    }

    pub(crate) async fn _did_change_workspace_folders(
        &self,
        params: DidChangeWorkspaceFoldersParams,
    ) {
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
            let ws_changed = {
                let mut index = self.workspace_index.lock();
                let mut docs = self.workspace_documents.lock();
                let stale: Vec<String> = docs
                    .keys()
                    .filter(|uri| uri_within_any(uri, &removed))
                    .cloned()
                    .collect();
                let mut changed: Vec<witcherscript_language::resolve::ObservedKey> = Vec::new();
                for uri in stale {
                    changed.extend(index.remove_document(&uri));
                    docs.remove(&uri);
                }
                changed
            };
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
    }
}
