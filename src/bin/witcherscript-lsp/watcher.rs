use std::collections::HashSet;
use std::path::PathBuf;

use lsp_types::request::RegisterCapability;
use lsp_types::{
    DidChangeWatchedFilesRegistrationOptions, FileChangeType, FileEvent, FileSystemWatcher,
    GlobPattern, Registration, RegistrationParams,
};
use tracing::{debug, warn};
use witcherscript_language::document::parse_document;
use witcherscript_language::files::{
    canonical_uri, is_witcherscript_file, read_script_file, ExcludeFilter,
};

use crate::backend::Backend;

pub(crate) fn event_touches_legacy_dir(event: &FileEvent, legacy_dirs: &[PathBuf]) -> bool {
    if legacy_dirs.is_empty() {
        return false;
    }
    let Ok(path) = event.uri.to_file_path() else {
        return false;
    };
    legacy_dirs.iter().any(|dir| path.starts_with(dir))
}

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
    match event.typ {
        // A delete must drop the file even while it is open: the file is gone.
        FileChangeType::DELETED => Some(WatchedEvent::Remove { canonical }),
        FileChangeType::CREATED | FileChangeType::CHANGED => {
            if open_canonical.contains(&canonical) {
                return None;
            }
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

    pub(crate) fn apply_watched_file_events(&self, events: Vec<FileEvent>) {
        let open_canonical: HashSet<String> = {
            let documents = self.documents.lock();
            documents.keys().filter_map(canonical_uri).collect()
        };
        let filter = self.exclude_filter();
        let legacy_dirs = self.effective_legacy_dirs();

        let mut updates: Vec<(String, witcherscript_language::document::ParsedDocument)> =
            Vec::new();
        let mut removals: Vec<String> = Vec::new();
        let mut legacy_map_refresh = false;

        for event in events {
            let touches_legacy = event_touches_legacy_dir(&event, &legacy_dirs);
            let Some(decision) = classify_watched_event(&event, &open_canonical, &filter) else {
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
                    if touches_legacy {
                        legacy_map_refresh = true;
                    }
                }
                WatchedEvent::Remove { canonical } => {
                    debug!(canonical = %canonical, "watched file removed");
                    removals.push(canonical);
                    if touches_legacy {
                        legacy_map_refresh = true;
                    }
                }
            }
        }

        let had_updates = !updates.is_empty();
        let had_removals = !removals.is_empty();
        let mut invalidated = HashSet::new();
        if had_updates || had_removals {
            {
                let mut index = self.workspace_index.lock();
                let mut docs = self.workspace_documents.lock();
                let mut known = self.workspace_known_files.lock();
                for (canonical, document) in updates {
                    invalidated.extend(index.update_document(canonical.as_str(), &document));
                    known.insert(canonical.clone());
                    docs.insert(canonical, document);
                }
                for canonical in &removals {
                    invalidated.extend(index.remove_document(canonical));
                    known.remove(canonical);
                    docs.remove(canonical);
                }
            }
            self.evict_cache_entries(&invalidated);
        }

        if legacy_map_refresh {
            self.refresh_legacy_override_maps();
            self.publish_legacy_script_status();
            self.publish_file_scope_status();
        }

        if had_updates || had_removals || legacy_map_refresh {
            self.publish_open_diagnostics();
        }
    }
}
