use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::backend::{build_symbol_db, diagnostics_document_set, Backend};
use crate::config::DiagnosticsScope;
use crate::convert::{lsp_diagnostics, lsp_workspace_diagnostic};
use crate::cst_cache::cst_diagnostics_with_cache;
use crate::file_scope::{classify_file_scope, FileScope};
use crate::file_scope_status::{FileScopeStatusNotification, FileScopeStatusParams};
use crate::legacy_status::{LegacyScriptStatusNotification, LegacyScriptStatusParams};
use lsp_types::{
    Diagnostic, FullDocumentDiagnosticReport, UnchangedDocumentDiagnosticReport, Url,
    WorkspaceDiagnosticReport, WorkspaceDocumentDiagnosticReport,
    WorkspaceFullDocumentDiagnosticReport, WorkspaceUnchangedDocumentDiagnosticReport,
};
use tracing::debug;

use witcherscript_language::diagnostics::{
    collect_base_script_conflict_diagnostics, collect_duplicate_local_diagnostics,
    collect_duplicate_symbol_diagnostics, collect_shadowing_diagnostics, WorkspaceDiagnostic,
};
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::line_index::SourceRange;
use witcherscript_language::resolve::WorkspaceIndex;
use witcherscript_language::script_env::ScriptEnvironment;

struct DiagnosticsBundle {
    dup: HashMap<String, Vec<WorkspaceDiagnostic>>,
    shadow: HashMap<String, Vec<WorkspaceDiagnostic>>,
    dup_local: HashMap<String, Vec<WorkspaceDiagnostic>>,
    base_conflict: HashMap<String, Vec<WorkspaceDiagnostic>>,
}

fn collect_workspace_diagnostics(
    workspace: &WorkspaceIndex,
    loose: &WorkspaceIndex,
    base: &WorkspaceIndex,
    env: &ScriptEnvironment,
    legacy_dirs: &[PathBuf],
    should_continue: &dyn Fn() -> bool,
) -> Option<DiagnosticsBundle> {
    let mut dup = tracing::debug_span!("dup_symbols")
        .in_scope(|| collect_duplicate_symbol_diagnostics(workspace));
    if !should_continue() {
        return None;
    }
    let mut shadow = tracing::debug_span!("shadowing")
        .in_scope(|| collect_shadowing_diagnostics(workspace, env));
    if !should_continue() {
        return None;
    }
    let mut dup_local = tracing::debug_span!("dup_locals")
        .in_scope(|| collect_duplicate_local_diagnostics(workspace));
    if !should_continue() {
        return None;
    }
    let base_conflict = tracing::debug_span!("base_script_conflict")
        .in_scope(|| collect_base_script_conflict_diagnostics(workspace, base, legacy_dirs));
    if !should_continue() {
        return None;
    }
    // Loose files compile in isolation; duplicates among them are still real.
    dup.extend(collect_duplicate_symbol_diagnostics(loose));
    shadow.extend(collect_shadowing_diagnostics(loose, env));
    dup_local.extend(collect_duplicate_local_diagnostics(loose));
    Some(DiagnosticsBundle {
        dup,
        shadow,
        dup_local,
        base_conflict,
    })
}

fn assemble_diagnostics_for_key(
    diag_key: &str,
    bundle: &DiagnosticsBundle,
    cst_by_uri: &HashMap<String, Vec<WorkspaceDiagnostic>>,
    document: &ParsedDocument,
) -> Vec<Diagnostic> {
    let mut diagnostics = lsp_diagnostics(document);
    let base_conflicts = bundle
        .base_conflict
        .get(diag_key)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if let Some(dups) = bundle.dup.get(diag_key) {
        diagnostics.extend(
            duplicates_not_explained_by_conflict(dups, base_conflicts)
                .map(lsp_workspace_diagnostic),
        );
    }
    diagnostics.extend(base_conflicts.iter().map(lsp_workspace_diagnostic));
    if let Some(shadows) = bundle.shadow.get(diag_key) {
        diagnostics.extend(shadows.iter().map(lsp_workspace_diagnostic));
    }
    if let Some(dup_locals) = bundle.dup_local.get(diag_key) {
        diagnostics.extend(dup_locals.iter().map(lsp_workspace_diagnostic));
    }
    if let Some(cst_diags) = cst_by_uri.get(diag_key) {
        diagnostics.extend(cst_diags.iter().map(lsp_workspace_diagnostic));
    }
    diagnostics
}

fn result_id_for(
    parse_version: u64,
    workspace_surface: u64,
    base_surface: u64,
    env_version: u64,
    legacy_db_generation: u64,
) -> String {
    format!(
        "{}-{:x}-{:x}-{:x}-{:x}",
        parse_version, workspace_surface, base_surface, env_version, legacy_db_generation,
    )
}

// A file is published under its canonical URI so its key is stable whether or not it is open.
pub(crate) fn publish_url(diag_key: &str) -> Option<Url> {
    let parsed = Url::parse(diag_key).ok()?;
    match canonical_uri(&parsed) {
        Some(canonical) => Url::parse(&canonical).ok(),
        None => Some(parsed),
    }
}

impl Backend {
    pub(crate) fn compute_diagnostics_for_uri(
        &self,
        uri: &Url,
        document: &witcherscript_language::document::ParsedDocument,
        version: u64,
    ) -> Option<(Vec<Diagnostic>, String)> {
        let started_at = Instant::now();
        debug!(
            op = "compute_diagnostics_for_uri",
            uri = %uri,
            "start",
        );
        let is_loose = self.file_scope_of(uri).is_loose();
        let legacy_dirs = self.effective_legacy_dirs();
        let snap = self.snapshot();
        let workspace = &snap.workspace_index;
        let loose = &snap.loose_index;
        let base = &snap.base_scripts_index;
        let env = &snap.script_env;
        let mut cache = self.cst_diag_cache.lock();
        if self.diagnostic_version.load(Ordering::Acquire) != version {
            return None;
        }
        let suppressed = &snap.suppressed_base_uris;
        let filtered = snap.filtered_base_catalogs.as_deref();

        let version_check = || self.diagnostic_version.load(Ordering::Acquire) == version;
        let bundle = collect_workspace_diagnostics(
            workspace,
            loose,
            base,
            env,
            &legacy_dirs,
            &version_check,
        )?;
        if !version_check() {
            return None;
        }

        let mut loose_uri_strs: HashSet<String> = HashSet::new();
        if is_loose {
            loose_uri_strs.insert(uri.to_string());
        }
        let fingerprint = self.db_fingerprint(base, env);

        let ws_db = build_symbol_db(
            workspace,
            base,
            env,
            self.builtins_index.as_ref(),
            suppressed,
            filtered,
        );
        let loose_db = build_symbol_db(
            loose,
            base,
            env,
            self.builtins_index.as_ref(),
            suppressed,
            filtered,
        );

        let mut diag_docs: HashMap<String, &ParsedDocument> = HashMap::new();
        diag_docs.insert(uri.to_string(), document);

        let diagnostic_version = self.diagnostic_version.clone();
        let should_continue = move || diagnostic_version.load(Ordering::Acquire) == version;
        let cst = cst_diagnostics_with_cache(
            &diag_docs,
            &ws_db,
            Some((&loose_db, &loose_uri_strs)),
            fingerprint,
            &mut cache,
            &should_continue,
        );
        if cst.cancelled {
            return None;
        }

        let key = uri.to_string();
        let diagnostics =
            assemble_diagnostics_for_key(key.as_str(), &bundle, &cst.by_uri, document);

        let result_id = result_id_for(
            document.parse_version,
            workspace.surface_hash(),
            base.surface_hash(),
            env.version(),
            fingerprint.legacy_db_generation,
        );
        debug!(
            op = "compute_diagnostics_for_uri",
            uri = %uri,
            diagnostics = diagnostics.len(),
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        Some((diagnostics, result_id))
    }

    pub(crate) fn compute_workspace_diagnostic_report(
        &self,
        previous: HashMap<String, String>,
        version: u64,
    ) -> Option<WorkspaceDiagnosticReport> {
        let started_at = Instant::now();
        debug!(op = "compute_workspace_diagnostic_report", version, "start");

        let cfg = self.config.load();
        if matches!(cfg.diagnostics_scope, DiagnosticsScope::None) {
            return Some(WorkspaceDiagnosticReport { items: Vec::new() });
        }
        let whole_workspace = matches!(cfg.diagnostics_scope, DiagnosticsScope::Workspace);

        let snap = self.snapshot();
        let legacy_dirs = self.effective_legacy_dirs();
        let loose_uris = self.loose_open_uris(&snap.documents);
        let workspace = &snap.workspace_index;
        let loose = &snap.loose_index;
        let base = &snap.base_scripts_index;
        let env = &snap.script_env;
        let suppressed = &snap.suppressed_base_uris;
        let filtered = snap.filtered_base_catalogs.as_deref();

        let mut cache = self.cst_diag_cache.lock();
        if self.diagnostic_version.load(Ordering::Acquire) != version {
            return None;
        }

        let diag_docs =
            diagnostics_document_set(&snap.workspace_documents, &snap.documents, whole_workspace);

        let version_check = || self.diagnostic_version.load(Ordering::Acquire) == version;
        let bundle = collect_workspace_diagnostics(
            workspace,
            loose,
            base,
            env,
            &legacy_dirs,
            &version_check,
        )?;
        if !version_check() {
            return None;
        }

        let fingerprint = self.db_fingerprint(base, env);
        let loose_uri_strs: HashSet<String> = loose_uris.iter().map(|u| u.to_string()).collect();
        let ws_db = build_symbol_db(
            workspace,
            base,
            env,
            self.builtins_index.as_ref(),
            suppressed,
            filtered,
        );
        let loose_db = build_symbol_db(
            loose,
            base,
            env,
            self.builtins_index.as_ref(),
            suppressed,
            filtered,
        );

        let diagnostic_version = self.diagnostic_version.clone();
        let should_continue = move || diagnostic_version.load(Ordering::Acquire) == version;
        let cst = cst_diagnostics_with_cache(
            &diag_docs,
            &ws_db,
            Some((&loose_db, &loose_uri_strs)),
            fingerprint,
            &mut cache,
            &should_continue,
        );
        if cst.cancelled {
            return None;
        }
        let cst_stats = cst.stats;

        {
            let mut loose_subs = self.loose_subscriptions.lock();
            let mut workspace_subs = self.workspace_subscriptions.lock();
            for (uri, observations) in cst.new_subscriptions {
                if loose_uri_strs.contains(&uri) {
                    loose_subs.register(&uri, observations);
                } else {
                    workspace_subs.register(&uri, observations);
                }
            }
        }

        let workspace_surface = workspace.surface_hash();
        let base_surface = base.surface_hash();
        let env_version = env.version();
        let mut items: Vec<WorkspaceDocumentDiagnosticReport> = Vec::with_capacity(diag_docs.len());
        for (diag_key, document) in diag_docs.iter() {
            let Some(publish_uri) = publish_url(diag_key) else {
                continue;
            };
            let result_id = result_id_for(
                document.parse_version,
                workspace_surface,
                base_surface,
                env_version,
                fingerprint.legacy_db_generation,
            );
            if previous.get(diag_key) == Some(&result_id) {
                items.push(WorkspaceDocumentDiagnosticReport::Unchanged(
                    WorkspaceUnchangedDocumentDiagnosticReport {
                        uri: publish_uri,
                        version: None,
                        unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                            result_id,
                        },
                    },
                ));
                continue;
            }
            let diagnostics =
                assemble_diagnostics_for_key(diag_key.as_str(), &bundle, &cst.by_uri, document);
            items.push(WorkspaceDocumentDiagnosticReport::Full(
                WorkspaceFullDocumentDiagnosticReport {
                    uri: publish_uri,
                    version: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: Some(result_id),
                        items: diagnostics,
                    },
                },
            ));
        }

        debug!(
            op = "compute_workspace_diagnostic_report",
            version,
            items = items.len(),
            cst_cache_hits = cst_stats.hits,
            cst_cache_misses = cst_stats.misses,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        Some(WorkspaceDiagnosticReport { items })
    }

    pub(crate) fn publish_legacy_script_status(&self) {
        let to_send: Vec<LegacyScriptStatusParams> = {
            let documents = self.snapshot().documents.clone();
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
            // notify failure means the client disconnected; nothing to recover.
            let _ = self.client.notify::<LegacyScriptStatusNotification>(params);
        }
    }

    pub(crate) fn publish_file_scope_status(&self) {
        let to_send: Vec<FileScopeStatusParams> = {
            let documents = self.snapshot().documents.clone();
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
            // notify failure means the client disconnected; nothing to recover.
            let _ = self.client.notify::<FileScopeStatusNotification>(params);
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
