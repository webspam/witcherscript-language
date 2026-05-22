use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Instant;

use lsp_types::notification::PublishDiagnostics;
use lsp_types::{Diagnostic, PublishDiagnosticsParams, Url};
use tracing::trace;
use witcherscript_language::diagnostics::{
    collect_base_script_conflict_diagnostics, collect_duplicate_local_diagnostics,
    collect_duplicate_symbol_diagnostics, collect_shadowing_diagnostics, WorkspaceDiagnostic,
};
use witcherscript_language::files::canonical_uri;
use witcherscript_language::line_index::SourceRange;
use witcherscript_language::resolve::SymbolDb;

use crate::backend::{diagnostics_document_set, Backend};
use crate::config::DiagnosticsScope;
use crate::convert::{lsp_diagnostics, lsp_workspace_diagnostic};
use crate::cst_cache::{cst_diagnostics_with_cache, DbFingerprint};
use crate::file_scope::{classify_file_scope, FileScope};
use crate::file_scope_status::{FileScopeStatusNotification, FileScopeStatusParams};
use crate::legacy_status::{LegacyScriptStatusNotification, LegacyScriptStatusParams};

// A file is published under its canonical URI so its key is stable whether or not it is open.
pub(crate) fn publish_url(diag_key: &str) -> Option<Url> {
    let parsed = Url::parse(diag_key).ok()?;
    match canonical_uri(&parsed) {
        Some(canonical) => Url::parse(&canonical).ok(),
        None => Some(parsed),
    }
}

impl Backend {
    #[tracing::instrument(skip(self), level = "debug")]
    pub(crate) async fn publish_open_diagnostics(&self) {
        let cfg = self.config.load();
        if !cfg.diagnostics_enabled {
            return;
        }

        if !self.initial_index_done.load(Ordering::Acquire) {
            self.publish_syntactic_only().await;
            return;
        }

        let whole_workspace = matches!(cfg.diagnostics_scope, DiagnosticsScope::Workspace);
        let start = Instant::now();

        let documents = self.documents.lock().await;
        let legacy_dirs = self.effective_legacy_dirs().await;
        let loose_uris = self.loose_open_uris(&documents).await;

        let (to_publish, cst_stats): (Vec<(Url, Vec<Diagnostic>)>, _) = {
            let mut workspace = self.workspace_index.lock().await;
            let mut loose = self.loose_index.lock().await;
            let base = self.base_scripts_index.lock().await;
            let env = self.script_env.lock().await;
            let mut cache = self.cst_diag_cache.lock().await;
            let workspace_documents = self.workspace_documents.lock().await;

            let diag_docs =
                diagnostics_document_set(&workspace_documents, &documents, whole_workspace);

            let mut dup = tracing::debug_span!("dup_symbols")
                .in_scope(|| collect_duplicate_symbol_diagnostics(&workspace));
            let mut shadow = tracing::debug_span!("shadowing")
                .in_scope(|| collect_shadowing_diagnostics(&workspace, &env));
            let mut dup_local = tracing::debug_span!("dup_locals")
                .in_scope(|| collect_duplicate_local_diagnostics(&workspace));
            let base_conflict = tracing::debug_span!("base_script_conflict").in_scope(|| {
                collect_base_script_conflict_diagnostics(&workspace, &base, &legacy_dirs)
            });
            // Loose files compile in isolation; duplicates among them are still real.
            dup.extend(collect_duplicate_symbol_diagnostics(&loose));
            shadow.extend(collect_shadowing_diagnostics(&loose, &env));
            dup_local.extend(collect_duplicate_local_diagnostics(&loose));

            let fingerprint = DbFingerprint {
                base_surface: base.surface_hash(),
                env: env.version(),
            };
            let loose_uri_strs: HashSet<String> =
                loose_uris.iter().map(|u| u.to_string()).collect();
            let cst = {
                let ws_db = SymbolDb::new(&workspace, &base)
                    .with_script_env(&env)
                    .with_builtins(&self.builtins_index);
                let loose_db = SymbolDb::new(&loose, &base)
                    .with_script_env(&env)
                    .with_builtins(&self.builtins_index);
                tracing::debug_span!("cst_diagnostics", docs = diag_docs.len()).in_scope(|| {
                    cst_diagnostics_with_cache(
                        &diag_docs,
                        &ws_db,
                        Some((&loose_db, &loose_uri_strs)),
                        fingerprint,
                        &mut cache,
                    )
                })
            };
            let cst_stats = cst.stats;
            for (uri, observations) in cst.new_subscriptions {
                if loose_uri_strs.contains(&uri) {
                    loose.register_subscription(&uri, observations);
                } else {
                    workspace.register_subscription(&uri, observations);
                }
            }

            let mut published = self.published_diagnostics.lock().await;
            let mut current: HashSet<Url> = HashSet::new();
            let mut list: Vec<(Url, Vec<Diagnostic>)> = Vec::new();
            for (diag_key, document) in diag_docs.iter() {
                let mut diagnostics = lsp_diagnostics(document);
                let base_conflicts = base_conflict
                    .get(diag_key.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if let Some(dups) = dup.get(diag_key.as_str()) {
                    diagnostics.extend(
                        duplicates_not_explained_by_conflict(dups, base_conflicts)
                            .map(lsp_workspace_diagnostic),
                    );
                }
                diagnostics.extend(base_conflicts.iter().map(lsp_workspace_diagnostic));
                if let Some(shadows) = shadow.get(diag_key.as_str()) {
                    diagnostics.extend(shadows.iter().map(lsp_workspace_diagnostic));
                }
                if let Some(dup_locals) = dup_local.get(diag_key.as_str()) {
                    diagnostics.extend(dup_locals.iter().map(lsp_workspace_diagnostic));
                }
                if let Some(cst_diags) = cst.by_uri.get(diag_key.as_str()) {
                    diagnostics.extend(cst_diags.iter().map(lsp_workspace_diagnostic));
                }
                let Some(publish_uri) = publish_url(diag_key) else {
                    continue;
                };
                current.insert(publish_uri.clone());
                if published.get(&publish_uri) == Some(&diagnostics) {
                    continue;
                }
                published.insert(publish_uri.clone(), diagnostics.clone());
                list.push((publish_uri, diagnostics));
            }

            // A file that left the diagnosed set (closed, deleted, or scope narrowed) has its diagnostics retracted.
            let stale: Vec<Url> = published
                .keys()
                .filter(|uri| !current.contains(*uri))
                .cloned()
                .collect();
            for uri in stale {
                published.remove(&uri);
                list.push((uri, Vec::new()));
            }
            (list, cst_stats)
        };

        drop(documents);

        let republished = to_publish.len();
        for (uri, diagnostics) in to_publish {
            let _ = self
                .client
                .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                    uri,
                    diagnostics,
                    version: None,
                });
        }

        trace!(
            republished,
            cst_cache_hits = cst_stats.hits,
            cst_cache_misses = cst_stats.misses,
            total_us = start.elapsed().as_micros(),
            "recomputed workspace diagnostics"
        );
    }

    async fn publish_syntactic_only(&self) {
        let to_publish: Vec<(Url, Vec<Diagnostic>)> = {
            let documents = self.documents.lock().await;
            let mut published = self.published_diagnostics.lock().await;
            let mut list = Vec::new();
            for (uri, document) in documents.iter() {
                let diagnostics = lsp_diagnostics(document);
                let Some(publish_uri) = publish_url(uri.as_str()) else {
                    continue;
                };
                if published.get(&publish_uri) == Some(&diagnostics) {
                    continue;
                }
                published.insert(publish_uri.clone(), diagnostics.clone());
                list.push((publish_uri, diagnostics));
            }
            list
        };

        for (uri, diagnostics) in to_publish {
            let _ = self
                .client
                .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                    uri,
                    diagnostics,
                    version: None,
                });
        }
    }

    pub(crate) async fn publish_legacy_script_status(&self) {
        let to_send: Vec<LegacyScriptStatusParams> = {
            let documents = self.documents.lock().await;
            let replacements = self.legacy_replacements.lock().await;
            let mut sent = self.sent_legacy_status.lock().await;
            let mut list = Vec::new();
            for uri in documents.keys() {
                let replaced =
                    canonical_uri(uri).and_then(|canon| replacements.get(&canon).cloned());
                let params = LegacyScriptStatusParams::new(uri.to_string(), replaced);
                if sent.get(uri) == Some(&params) {
                    continue;
                }
                sent.insert(uri.clone(), params.clone());
                list.push(params);
            }
            list
        };
        for params in to_send {
            let _ = self.client.notify::<LegacyScriptStatusNotification>(params);
        }
    }

    pub(crate) async fn publish_file_scope_status(&self) {
        let to_send: Vec<FileScopeStatusParams> = {
            let documents = self.documents.lock().await;
            let roots = self.workspace_roots.lock().await.clone();
            let legacy_dirs = self.effective_legacy_dirs().await;
            let game_dir = self.base_scripts_path.lock().await.clone();
            let additional = self.additional_script_dirs.lock().await.clone();
            let replacements = self.legacy_replacements.lock().await;
            let mut sent = self.sent_file_scope_status.lock().await;
            let mut list = Vec::new();
            for uri in documents.keys() {
                let scope = classify_file_scope(
                    uri,
                    &roots,
                    &legacy_dirs,
                    &replacements,
                    game_dir.as_deref(),
                    &additional,
                );
                let replaced_script_path = if matches!(scope, FileScope::LegacyOverride) {
                    canonical_uri(uri).and_then(|canon| replacements.get(&canon).cloned())
                } else {
                    None
                };
                let params = FileScopeStatusParams {
                    uri: uri.to_string(),
                    scope,
                    replaced_script_path,
                };
                if sent.get(uri) == Some(&params) {
                    continue;
                }
                sent.insert(uri.clone(), params.clone());
                list.push(params);
            }
            list
        };
        for params in to_send {
            let _ = self.client.notify::<FileScopeStatusNotification>(params);
        }
    }

    pub(crate) async fn reconcile_published_diagnostics(&self) {
        if self.config.load().diagnostics_enabled {
            self.publish_open_diagnostics().await;
        } else {
            let uris: Vec<Url> = {
                let mut published = self.published_diagnostics.lock().await;
                let keys: Vec<Url> = published.keys().cloned().collect();
                published.clear();
                keys
            };
            for uri in uris {
                let _ = self
                    .client
                    .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                        uri,
                        diagnostics: Vec::new(),
                        version: None,
                    });
            }
        }
    }
}

// A legacy override file shows the friendlier base-script conflict error, not both.
fn duplicates_not_explained_by_conflict<'a>(
    duplicates: &'a [WorkspaceDiagnostic],
    conflicts: &[WorkspaceDiagnostic],
) -> impl Iterator<Item = &'a WorkspaceDiagnostic> {
    let conflict_ranges: Vec<SourceRange> = conflicts.iter().map(|c| c.range).collect();
    duplicates
        .iter()
        .filter(move |d| !conflict_ranges.contains(&d.range))
}

#[cfg(test)]
mod tests {
    use super::duplicates_not_explained_by_conflict;
    use witcherscript_language::diagnostics::{Severity, WorkspaceDiagnostic};
    use witcherscript_language::line_index::{SourcePosition, SourceRange};

    fn diag(kind: &str, line: u32) -> WorkspaceDiagnostic {
        let pos = SourcePosition { line, character: 0 };
        WorkspaceDiagnostic {
            kind: kind.to_string(),
            message: String::new(),
            severity: Severity::Error,
            range: SourceRange {
                start: pos,
                end: pos,
            },
            related: vec![],
            data: None,
        }
    }

    #[test]
    fn drops_duplicate_where_a_conflict_covers_the_same_declaration() {
        let dups = vec![diag("duplicate_symbol", 0), diag("duplicate_symbol", 5)];
        let conflicts = vec![diag("base_script_conflict", 0)];
        let kept: Vec<u32> = duplicates_not_explained_by_conflict(&dups, &conflicts)
            .map(|d| d.range.start.line)
            .collect();
        assert_eq!(
            kept,
            vec![5],
            "the duplicate at the conflict's declaration is suppressed"
        );
    }

    #[test]
    fn keeps_every_duplicate_when_there_are_no_conflicts() {
        let dups = vec![diag("duplicate_symbol", 0), diag("duplicate_symbol", 5)];
        let kept = duplicates_not_explained_by_conflict(&dups, &[]).count();
        assert_eq!(kept, 2);
    }
}
