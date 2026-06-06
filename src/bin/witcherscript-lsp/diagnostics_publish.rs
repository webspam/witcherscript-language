use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use crate::backend::{build_symbol_db, diagnostics_document_set, Backend};
use crate::config::DiagnosticsScope;
use crate::convert::{lsp_diagnostics, lsp_workspace_diagnostic};
use crate::cst_cache::{cst_diagnostics_with_cache, CstDiagnosticsResult, DbFingerprint};
use crate::file_scope::{classify_file_scope, FileScope};
use crate::file_scope_status::{FileScopeStatusNotification, FileScopeStatusParams};
use crate::legacy_status::{LegacyScriptStatusNotification, LegacyScriptStatusParams};
use lsp_types::{
    Diagnostic, FullDocumentDiagnosticReport, UnchangedDocumentDiagnosticReport, Url,
    WorkspaceDiagnosticReport, WorkspaceDocumentDiagnosticReport,
    WorkspaceFullDocumentDiagnosticReport, WorkspaceUnchangedDocumentDiagnosticReport,
};
use tracing::{debug, trace};

use witcherscript_language::diagnostics::{
    collect_base_script_conflict_diagnostics, collect_duplicate_local_diagnostics,
    collect_duplicate_symbol_diagnostics, collect_shadowing_diagnostics, WorkspaceDiagnostic,
};
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::line_index::SourceRange;
use witcherscript_language::resolve::WorkspaceIndex;
use witcherscript_language::script_env::ScriptEnvironment;

#[derive(Debug)]
pub(crate) struct DiagnosticsBundle {
    dup: HashMap<String, Vec<WorkspaceDiagnostic>>,
    shadow: HashMap<String, Vec<WorkspaceDiagnostic>>,
    dup_local: HashMap<String, Vec<WorkspaceDiagnostic>>,
    base_conflict: HashMap<String, Vec<WorkspaceDiagnostic>>,
}

// The bundle is a pure function of these inputs; a key match means a cached bundle is still valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BundleFingerprint {
    // Generations, not surface hashes: dup_local/shadow read locals, which surface hashing drops.
    workspace_generation: u64,
    loose_generation: u64,
    base_surface: u64,
    env: u64,
    legacy_dirs_hash: u64,
}

pub(crate) type CachedBundle = (BundleFingerprint, Arc<DiagnosticsBundle>);

fn hash_legacy_dirs(legacy_dirs: &[PathBuf]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    legacy_dirs.hash(&mut hasher);
    hasher.finish()
}

fn collect_workspace_diagnostics(
    workspace: &WorkspaceIndex,
    loose: &WorkspaceIndex,
    base: &WorkspaceIndex,
    env: &ScriptEnvironment,
    legacy_dirs: &[PathBuf],
    should_continue: &dyn Fn() -> bool,
) -> Option<DiagnosticsBundle> {
    let mut dup = tracing::trace_span!("dup_symbols")
        .in_scope(|| collect_duplicate_symbol_diagnostics(workspace));
    if !should_continue() {
        return None;
    }
    let mut shadow = tracing::trace_span!("shadowing")
        .in_scope(|| collect_shadowing_diagnostics(workspace, env));
    if !should_continue() {
        return None;
    }
    let mut dup_local = tracing::trace_span!("dup_locals")
        .in_scope(|| collect_duplicate_local_diagnostics(workspace));
    if !should_continue() {
        return None;
    }
    let base_conflict = tracing::trace_span!("base_script_conflict")
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

// Clients treat omission as "no change", so a file that left the diagnosed set needs an explicit empty Full.
fn clearing_items_for<I, S>(uris: I) -> Vec<WorkspaceDocumentDiagnosticReport>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    uris.into_iter()
        .filter_map(|s| Url::parse(s.as_ref()).ok())
        .map(|parsed| {
            WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
                uri: parsed,
                version: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items: Vec::new(),
                },
            })
        })
        .collect()
}

// A file is published under its canonical URI so its key is stable whether or not it is open.
pub(crate) fn publish_url(diag_key: &str) -> Option<Url> {
    let parsed = Url::parse(diag_key).ok()?;
    Url::parse(&canonical_uri(&parsed)).ok()
}

struct PullCompute {
    bundle: Arc<DiagnosticsBundle>,
    cst: CstDiagnosticsResult,
    workspace_surface: u64,
    base_surface: u64,
    env_version: u64,
    fingerprint: DbFingerprint,
}

impl Backend {
    #[allow(clippy::too_many_arguments)]
    fn workspace_bundle(
        &self,
        fingerprint: BundleFingerprint,
        workspace: &WorkspaceIndex,
        loose: &WorkspaceIndex,
        base: &WorkspaceIndex,
        env: &ScriptEnvironment,
        legacy_dirs: &[PathBuf],
        should_continue: &dyn Fn() -> bool,
    ) -> Option<Arc<DiagnosticsBundle>> {
        if let Some((cached, bundle)) = self.diag_bundle_cache.lock().as_ref() {
            if *cached == fingerprint {
                return Some(bundle.clone());
            }
        }
        let bundle = Arc::new(collect_workspace_diagnostics(
            workspace,
            loose,
            base,
            env,
            legacy_dirs,
            should_continue,
        )?);
        *self.diag_bundle_cache.lock() = Some((fingerprint, bundle.clone()));
        Some(bundle)
    }

    fn run_pull_compute(
        &self,
        diag_docs: &HashMap<String, &ParsedDocument>,
        loose_uri_strs: &HashSet<String>,
        version: u64,
    ) -> Option<PullCompute> {
        let legacy_dirs = self.effective_legacy_dirs();
        let snap = self.snapshot();
        let workspace = &snap.workspace_index;
        let loose = &snap.loose_index;
        let base = &snap.base_scripts_index;
        let env = &snap.script_env;
        let suppressed = &snap.suppressed_base_uris;
        let filtered = snap.filtered_base_catalogs.as_deref();

        if self.state_version.load(Ordering::Acquire) != version {
            return None;
        }
        let version_check = || self.state_version.load(Ordering::Acquire) == version;

        let fingerprint = self.db_fingerprint(base, env);
        let bundle_fingerprint = BundleFingerprint {
            workspace_generation: workspace.generation(),
            loose_generation: loose.generation(),
            base_surface: fingerprint.base_surface,
            env: fingerprint.env,
            legacy_dirs_hash: hash_legacy_dirs(&legacy_dirs),
        };
        let bundle_start = Instant::now();
        let bundle = self.workspace_bundle(
            bundle_fingerprint,
            workspace,
            loose,
            base,
            env,
            &legacy_dirs,
            &version_check,
        )?;
        let bundle_ms = bundle_start.elapsed().as_millis();
        if !version_check() {
            return None;
        }

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

        let state_version = self.state_version.clone();
        let should_continue = move || state_version.load(Ordering::Acquire) == version;
        let cst_start = Instant::now();
        let cst = cst_diagnostics_with_cache(
            diag_docs,
            &ws_db,
            Some((&loose_db, loose_uri_strs)),
            fingerprint,
            &self.cst_diag_cache,
            &should_continue,
        );
        if cst.cancelled {
            return None;
        }
        debug!(
            op = "run_pull_compute",
            docs = diag_docs.len(),
            cache_hits = cst.stats.hits,
            cache_misses = cst.stats.misses,
            bundle_ms,
            cst_ms = cst_start.elapsed().as_millis(),
            "diagnostics compute phases"
        );

        Some(PullCompute {
            bundle,
            cst,
            workspace_surface: workspace.surface_hash(),
            base_surface: base.surface_hash(),
            env_version: env.version(),
            fingerprint,
        })
    }

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
        let key = canonical_uri(uri);
        let mut diag_docs: HashMap<String, &ParsedDocument> = HashMap::new();
        diag_docs.insert(key.clone(), document);
        let mut loose_uri_strs: HashSet<String> = HashSet::new();
        if self.file_scope_of(uri).is_loose() {
            loose_uri_strs.insert(key.clone());
        }

        let compute = self.run_pull_compute(&diag_docs, &loose_uri_strs, version)?;
        let diagnostics = assemble_diagnostics_for_key(
            key.as_str(),
            &compute.bundle,
            &compute.cst.by_uri,
            document,
        );
        let result_id = result_id_for(
            document.parse_version,
            compute.workspace_surface,
            compute.base_surface,
            compute.env_version,
            compute.fingerprint.legacy_db_generation,
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
        trace!(op = "compute_workspace_diagnostic_report", version, "start");

        let cfg = self.config.load();
        if matches!(cfg.diagnostics_scope, DiagnosticsScope::None) {
            return Some(WorkspaceDiagnosticReport {
                items: clearing_items_for(previous.keys()),
            });
        }
        let whole_workspace = matches!(cfg.diagnostics_scope, DiagnosticsScope::Workspace);

        let snap = self.snapshot();
        let loose_uri_strs: HashSet<String> = self
            .loose_open_uris(&snap.documents)
            .iter()
            .map(canonical_uri)
            .collect();
        let diag_docs =
            diagnostics_document_set(&snap.workspace_documents, &snap.documents, whole_workspace);

        let compute = self.run_pull_compute(&diag_docs, &loose_uri_strs, version)?;
        let cst_stats = compute.cst.stats;

        // Only a whole-workspace pull sees every file, so only it may drop cache entries for files that vanished.
        if whole_workspace {
            self.cst_diag_cache
                .lock()
                .retain(|uri, _| diag_docs.contains_key(uri));
        }

        {
            let mut loose_subs = self.loose_subscriptions.lock();
            let mut workspace_subs = self.workspace_subscriptions.lock();
            for (uri, observations) in compute.cst.new_subscriptions {
                if loose_uri_strs.contains(&uri) {
                    loose_subs.register(&uri, observations);
                } else {
                    workspace_subs.register(&uri, observations);
                }
            }
        }

        let mut items: Vec<WorkspaceDocumentDiagnosticReport> = Vec::with_capacity(diag_docs.len());
        let mut emitted: HashSet<String> = HashSet::with_capacity(diag_docs.len());
        for (diag_key, document) in diag_docs.iter() {
            let Some(publish_uri) = publish_url(diag_key) else {
                continue;
            };
            let result_id = result_id_for(
                document.parse_version,
                compute.workspace_surface,
                compute.base_surface,
                compute.env_version,
                compute.fingerprint.legacy_db_generation,
            );
            let publish_key = publish_uri.to_string();
            emitted.insert(publish_key.clone());
            if previous.get(&publish_key) == Some(&result_id) {
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
            let diagnostics = assemble_diagnostics_for_key(
                diag_key.as_str(),
                &compute.bundle,
                &compute.cst.by_uri,
                document,
            );
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

        items.extend(clearing_items_for(
            previous.keys().filter(|k| !emitted.contains(k.as_str())),
        ));

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
            let replacements = self.legacy_replacements.load();
            let mut sent = self.sent_legacy_status.lock();
            let mut list = Vec::new();
            for uri in documents.keys() {
                let replaced = replacements.get(&canonical_uri(uri)).cloned();
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
            let roots = self.workspace_roots.load_full();
            let legacy_dirs = self.effective_legacy_dirs();
            let base_scripts_dir = self.base_scripts_dir();
            let additional = self.config.load().additional_script_dirs.clone();
            let replacements = self.legacy_replacements.load();
            let mut sent = self.sent_file_scope_status.lock();
            let mut list = Vec::new();
            for uri in documents.keys() {
                let scope = classify_file_scope(
                    uri,
                    &roots,
                    &legacy_dirs,
                    &replacements,
                    base_scripts_dir.as_deref(),
                    &additional,
                );
                let replaced_script_path = if matches!(scope, FileScope::LegacyOverride) {
                    replacements.get(&canonical_uri(uri)).cloned()
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
