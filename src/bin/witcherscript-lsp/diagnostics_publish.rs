use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::backend::{build_symbol_db, diagnostics_document_set, Backend};
use crate::config::DiagnosticsScope;
use crate::convert::{lsp_diagnostics, lsp_workspace_diagnostic};
use crate::cst_cache::cst_diagnostics_with_cache;
use crate::file_scope::{classify_file_scope, FileScope};
use crate::file_scope_status::{FileScopeStatusNotification, FileScopeStatusParams};
use crate::legacy_status::{LegacyScriptStatusNotification, LegacyScriptStatusParams};
use lsp_types::notification::PublishDiagnostics;
use lsp_types::{Diagnostic, PublishDiagnosticsParams, Url};
use tracing::trace;
use witcherscript_language::diagnostics::{
    collect_base_script_conflict_diagnostics, collect_duplicate_local_diagnostics,
    collect_duplicate_symbol_diagnostics, collect_shadowing_diagnostics, WorkspaceDiagnostic,
};
use witcherscript_language::files::canonical_uri;
use witcherscript_language::line_index::SourceRange;

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
    pub(crate) fn publish_open_diagnostics(&self, version: u64) {
        let cfg = self.config.load();
        if matches!(cfg.diagnostics_scope, DiagnosticsScope::None) {
            return;
        }

        if !self.initial_index_done.load(Ordering::Acquire) {
            self.publish_syntactic_only();
            return;
        }

        if self.diagnostic_version.load(Ordering::Acquire) != version {
            return;
        }

        let whole_workspace = matches!(cfg.diagnostics_scope, DiagnosticsScope::Workspace);
        let start = Instant::now();

        let documents = self.documents.lock();
        let legacy_dirs = self.effective_legacy_dirs();
        let loose_uris = self.loose_open_uris(&documents);

        let (to_publish, cst_stats): (Vec<(Url, Vec<Diagnostic>)>, _) = {
            let mut workspace = self.workspace_index.lock();
            let mut loose = self.loose_index.lock();
            let base = self.base_scripts_index.lock();
            let env = self.script_env.lock();
            let mut cache = self.cst_diag_cache.lock();
            let workspace_documents = self.workspace_documents.lock();

            if self.diagnostic_version.load(Ordering::Acquire) != version {
                return;
            }

            let diag_docs =
                diagnostics_document_set(&workspace_documents, &documents, whole_workspace);

            let mut dup = tracing::debug_span!("dup_symbols")
                .in_scope(|| collect_duplicate_symbol_diagnostics(&workspace));
            if self.diagnostic_version.load(Ordering::Acquire) != version {
                return;
            }
            let mut shadow = tracing::debug_span!("shadowing")
                .in_scope(|| collect_shadowing_diagnostics(&workspace, &env));
            if self.diagnostic_version.load(Ordering::Acquire) != version {
                return;
            }
            let mut dup_local = tracing::debug_span!("dup_locals")
                .in_scope(|| collect_duplicate_local_diagnostics(&workspace));
            if self.diagnostic_version.load(Ordering::Acquire) != version {
                return;
            }
            let base_conflict = tracing::debug_span!("base_script_conflict").in_scope(|| {
                collect_base_script_conflict_diagnostics(&workspace, &base, &legacy_dirs)
            });
            if self.diagnostic_version.load(Ordering::Acquire) != version {
                return;
            }
            // Loose files compile in isolation; duplicates among them are still real.
            dup.extend(collect_duplicate_symbol_diagnostics(&loose));
            shadow.extend(collect_shadowing_diagnostics(&loose, &env));
            dup_local.extend(collect_duplicate_local_diagnostics(&loose));

            if self.diagnostic_version.load(Ordering::Acquire) != version {
                return;
            }

            let fingerprint = self.db_fingerprint(&base, &env);
            let loose_uri_strs: HashSet<String> =
                loose_uris.iter().map(|u| u.to_string()).collect();
            let suppressed = self.suppressed_base_uris.lock();
            let filtered = self.filtered_base_catalogs.lock();
            let cst = {
                let ws_db = build_symbol_db(
                    &workspace,
                    &base,
                    &env,
                    self.builtins_index.as_ref(),
                    &suppressed,
                    filtered.as_ref(),
                );
                let loose_db = build_symbol_db(
                    &loose,
                    &base,
                    &env,
                    self.builtins_index.as_ref(),
                    &suppressed,
                    filtered.as_ref(),
                );
                let diagnostic_version = self.diagnostic_version.clone();
                let should_continue = move || diagnostic_version.load(Ordering::Acquire) == version;
                tracing::debug_span!("cst_diagnostics", docs = diag_docs.len()).in_scope(|| {
                    cst_diagnostics_with_cache(
                        &diag_docs,
                        &ws_db,
                        Some((&loose_db, &loose_uri_strs)),
                        fingerprint,
                        &mut cache,
                        &should_continue,
                    )
                })
            };
            if cst.cancelled {
                return;
            }
            let cst_stats = cst.stats;
            for (uri, observations) in cst.new_subscriptions {
                if loose_uri_strs.contains(&uri) {
                    loose.register_subscription(&uri, observations);
                } else {
                    workspace.register_subscription(&uri, observations);
                }
            }

            if self.diagnostic_version.load(Ordering::Acquire) != version {
                return;
            }

            let mut published = self.published_diagnostics.lock();
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

    // Pull diagnostics path: compute the current diagnostic vector + a fingerprint result_id
    // for one URI. The result_id changes whenever any input that could affect this file's
    // diagnostics changes (its own parse, workspace surface, base surface, env, legacy generation).
    pub(crate) fn compute_diagnostics_for_uri(
        &self,
        uri: &Url,
        document: &witcherscript_language::document::ParsedDocument,
    ) -> (Vec<Diagnostic>, String) {
        let is_loose = self.file_scope_of(uri).is_loose();
        let legacy_dirs = self.effective_legacy_dirs();
        let workspace = self.workspace_index.lock();
        let loose = self.loose_index.lock();
        let base = self.base_scripts_index.lock();
        let env = self.script_env.lock();
        let mut cache = self.cst_diag_cache.lock();
        let suppressed = self.suppressed_base_uris.lock();
        let filtered = self.filtered_base_catalogs.lock();

        let mut dup = collect_duplicate_symbol_diagnostics(&workspace);
        dup.extend(collect_duplicate_symbol_diagnostics(&loose));
        let mut shadow = collect_shadowing_diagnostics(&workspace, &env);
        shadow.extend(collect_shadowing_diagnostics(&loose, &env));
        let mut dup_local = collect_duplicate_local_diagnostics(&workspace);
        dup_local.extend(collect_duplicate_local_diagnostics(&loose));
        let base_conflict =
            collect_base_script_conflict_diagnostics(&workspace, &base, &legacy_dirs);

        let mut loose_uri_strs: HashSet<String> = HashSet::new();
        if is_loose {
            loose_uri_strs.insert(uri.to_string());
        }
        let fingerprint = self.db_fingerprint(&base, &env);

        let ws_db = build_symbol_db(
            &workspace,
            &base,
            &env,
            self.builtins_index.as_ref(),
            &suppressed,
            filtered.as_ref(),
        );
        let loose_db = build_symbol_db(
            &loose,
            &base,
            &env,
            self.builtins_index.as_ref(),
            &suppressed,
            filtered.as_ref(),
        );

        let mut diag_docs: std::collections::HashMap<
            String,
            &witcherscript_language::document::ParsedDocument,
        > = std::collections::HashMap::new();
        diag_docs.insert(uri.to_string(), document);

        let cst = cst_diagnostics_with_cache(
            &diag_docs,
            &ws_db,
            Some((&loose_db, &loose_uri_strs)),
            fingerprint,
            &mut cache,
            &|| true,
        );

        let key = uri.to_string();
        let mut diagnostics = lsp_diagnostics(document);
        let base_conflicts = base_conflict
            .get(key.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if let Some(dups) = dup.get(key.as_str()) {
            diagnostics.extend(
                duplicates_not_explained_by_conflict(dups, base_conflicts)
                    .map(lsp_workspace_diagnostic),
            );
        }
        diagnostics.extend(base_conflicts.iter().map(lsp_workspace_diagnostic));
        if let Some(shadows) = shadow.get(key.as_str()) {
            diagnostics.extend(shadows.iter().map(lsp_workspace_diagnostic));
        }
        if let Some(dup_locals) = dup_local.get(key.as_str()) {
            diagnostics.extend(dup_locals.iter().map(lsp_workspace_diagnostic));
        }
        if let Some(cst_diags) = cst.by_uri.get(key.as_str()) {
            diagnostics.extend(cst_diags.iter().map(lsp_workspace_diagnostic));
        }

        let result_id = format!(
            "{}-{:x}-{:x}-{:x}-{:x}",
            document.parse_version,
            workspace.surface_hash(),
            base.surface_hash(),
            env.version(),
            fingerprint.legacy_db_generation,
        );
        (diagnostics, result_id)
    }

    fn publish_syntactic_only(&self) {
        let to_publish: Vec<(Url, Vec<Diagnostic>)> = {
            let documents = self.documents.lock();
            let mut published = self.published_diagnostics.lock();
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

    pub(crate) fn publish_legacy_script_status(&self) {
        let to_send: Vec<LegacyScriptStatusParams> = {
            let documents = self.documents.lock();
            let replacements = self.legacy_replacements.lock();
            let mut sent = self.sent_legacy_status.lock();
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

    pub(crate) fn publish_file_scope_status(&self) {
        let to_send: Vec<FileScopeStatusParams> = {
            let documents = self.documents.lock();
            let roots = self.workspace_roots.lock().clone();
            let legacy_dirs = self.effective_legacy_dirs();
            let game_dir = self.base_scripts_path.lock().clone();
            let additional = self.additional_script_dirs.lock().clone();
            let replacements = self.legacy_replacements.lock();
            let mut sent = self.sent_file_scope_status.lock();
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

    pub(crate) fn reconcile_published_diagnostics(&self) {
        if !matches!(self.config.load().diagnostics_scope, DiagnosticsScope::None) {
            // Caller is already on a tokio task (config-change handler); skip the spawn so
            // tests that observe the published map directly after this call see the result.
            self.request_workspace_diagnostic_refresh();
            let version = self.diagnostic_version.fetch_add(1, Ordering::AcqRel) + 1;
            self.publish_open_diagnostics(version);
            return;
        }
        let uris: Vec<Url> = {
            let mut published = self.published_diagnostics.lock();
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
mod tests;
