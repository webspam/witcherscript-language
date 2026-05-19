use std::collections::HashSet;
use std::path::PathBuf;

use lsp_types::request::RegisterCapability;
use lsp_types::{
    DidChangeWatchedFilesRegistrationOptions, FileChangeType, FileEvent, FileSystemWatcher,
    GlobPattern, Registration, RegistrationParams,
};
use tracing::{debug, warn};
use witcherscript_language::document::parse_document;
use witcherscript_language::files::{is_witcherscript_file, read_script_file, ExcludeFilter};

use crate::backend::Backend;
use crate::convert::canonical_uri;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WatchedEvent {
    Upsert { canonical: String, path: PathBuf },
    Remove { canonical: String },
}

pub(crate) fn classify_watched_event(
    event: &FileEvent,
    open_canonical: &HashSet<String>,
    filter: &ExcludeFilter,
) -> Option<WatchedEvent> {
    let path = event.uri.to_file_path().ok()?;
    if !is_witcherscript_file(&path) {
        return None;
    }
    let canonical = canonical_uri(&event.uri)?;
    if open_canonical.contains(&canonical) {
        return None;
    }
    match event.typ {
        FileChangeType::DELETED => Some(WatchedEvent::Remove { canonical }),
        FileChangeType::CREATED | FileChangeType::CHANGED => {
            if filter.matches(&path) {
                return None;
            }
            Some(WatchedEvent::Upsert { canonical, path })
        }
        _ => None,
    }
}

impl Backend {
    pub(crate) async fn register_file_watchers(&self) {
        let watcher = FileSystemWatcher {
            glob_pattern: GlobPattern::String("**/*.ws".to_string()),
            kind: None,
        };
        let options = DidChangeWatchedFilesRegistrationOptions {
            watchers: vec![watcher],
        };
        let registration = Registration {
            id: "witcherscript-ws-files".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: serde_json::to_value(options).ok(),
        };
        if let Err(err) = self
            .client
            .request::<RegisterCapability>(RegistrationParams {
                registrations: vec![registration],
            })
            .await
        {
            warn!(
                error = %err,
                "failed to register file watcher; workspace index may go stale on external file changes"
            );
        }
    }

    pub(crate) async fn apply_watched_file_events(&self, events: Vec<FileEvent>) {
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock().await;
            documents.keys().filter_map(canonical_uri).collect()
        };
        let roots = self.workspace_roots.lock().await.clone();
        let filter = ExcludeFilter::new(&roots, &self.files_exclude.lock().await.clone());

        let mut updates: Vec<(String, witcherscript_language::document::ParsedDocument)> =
            Vec::new();
        let mut removals: Vec<String> = Vec::new();
        for event in &events {
            let Some(decision) = classify_watched_event(event, &open_canonical, &filter) else {
                continue;
            };
            match decision {
                WatchedEvent::Upsert { canonical, path } => {
                    let source = match read_script_file(&path) {
                        Ok(s) => s,
                        Err(err) => {
                            warn!(path = %path.display(), error = %err, "failed to read watched file");
                            continue;
                        }
                    };
                    let document = match parse_document(source) {
                        Ok(d) => d,
                        Err(err) => {
                            warn!(path = %path.display(), error = %err, "failed to parse watched file");
                            continue;
                        }
                    };
                    debug!(canonical = %canonical, "watched file upserted");
                    updates.push((canonical, document));
                }
                WatchedEvent::Remove { canonical } => {
                    debug!(canonical = %canonical, "watched file removed");
                    removals.push(canonical);
                }
            }
        }

        if updates.is_empty() && removals.is_empty() {
            return;
        }

        let invalidated = {
            let mut index = self.workspace_index.lock().await;
            let mut docs = self.workspace_documents.lock().await;
            let mut invalidated: HashSet<String> = HashSet::new();
            for (canonical, document) in updates {
                invalidated.extend(index.update_document(canonical.as_str(), &document));
                docs.insert(canonical, document);
            }
            for canonical in removals {
                invalidated.extend(index.remove_document(&canonical));
                docs.remove(&canonical);
            }
            invalidated
        };
        self.evict_cache_entries(&invalidated).await;

        self.publish_open_diagnostics().await;
    }
}
