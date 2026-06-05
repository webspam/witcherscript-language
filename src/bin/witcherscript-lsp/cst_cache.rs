use parking_lot::Mutex;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use witcherscript_language::diagnostics::{
    collect_cst_diagnostics_for_document, WorkspaceDiagnostic,
};
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::resolve::{ObservationSet, SymbolDb};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DbFingerprint {
    pub base_surface: u64,
    pub env: u64,
    pub legacy_db_generation: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CstCacheEntry {
    pub parse_version: u64,
    pub db_fingerprint: DbFingerprint,
    pub diagnostics: Vec<WorkspaceDiagnostic>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CstCacheStats {
    pub hits: usize,
    pub misses: usize,
}

pub(crate) struct CstDiagnosticsResult {
    pub by_uri: HashMap<String, Vec<WorkspaceDiagnostic>>,
    pub stats: CstCacheStats,
    pub new_subscriptions: Vec<(String, ObservationSet)>,
    pub cancelled: bool,
}

struct ComputedDoc {
    uri: String,
    parse_version: u64,
    diagnostics: Vec<WorkspaceDiagnostic>,
    observations: ObservationSet,
}

// Misses compute off-lock and in parallel across files; the lock only guards read and commit.
pub(crate) fn cst_diagnostics_with_cache(
    documents: &HashMap<String, &ParsedDocument>,
    db: &SymbolDb,
    loose: Option<(&SymbolDb, &HashSet<String>)>,
    fingerprint: DbFingerprint,
    cache: &Mutex<HashMap<String, CstCacheEntry>>,
    should_continue: &(dyn Fn() -> bool + Sync),
) -> CstDiagnosticsResult {
    let mut out: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();
    let mut hits = 0usize;
    let mut misses: Vec<(&String, &ParsedDocument)> = Vec::new();
    {
        let cache = cache.lock();
        for (uri, document) in documents.iter() {
            let cached = cache.get(uri).filter(|e| {
                e.parse_version == document.parse_version && e.db_fingerprint == fingerprint
            });
            if let Some(entry) = cached {
                hits += 1;
                if !entry.diagnostics.is_empty() {
                    out.insert(uri.clone(), entry.diagnostics.clone());
                }
            } else {
                misses.push((uri, *document));
            }
        }
    }
    let miss_count = misses.len();

    if !should_continue() {
        return CstDiagnosticsResult {
            by_uri: out,
            stats: CstCacheStats {
                hits,
                misses: miss_count,
            },
            new_subscriptions: Vec::new(),
            cancelled: true,
        };
    }

    // Set by any worker that sees the state version advance; the whole result is then discarded.
    let cancelled = AtomicBool::new(false);
    let compute_one = |uri: &String, document: &ParsedDocument| {
        if !should_continue() {
            cancelled.store(true, Ordering::Relaxed);
            return None;
        }
        let observations = Mutex::new(ObservationSet::default());
        let doc_db = match loose {
            Some((loose_db, loose_uris)) if loose_uris.contains(uri.as_str()) => loose_db,
            _ => db,
        };
        let recording_db = doc_db.with_observer(&observations);
        let diagnostics =
            tracing::trace_span!("cst_doc", uri = uri.as_str(), bytes = document.source.len())
                .in_scope(|| {
                    collect_cst_diagnostics_for_document(uri.as_str(), document, &recording_db)
                });
        Some(ComputedDoc {
            uri: uri.clone(),
            parse_version: document.parse_version,
            diagnostics,
            observations: observations.into_inner(),
        })
    };

    let computed: Vec<ComputedDoc> = misses
        .par_iter()
        .filter_map(|&(uri, document)| compute_one(uri, document))
        .collect();
    let cancelled = cancelled.load(Ordering::Relaxed);

    {
        let mut cache = cache.lock();
        for c in &computed {
            cache.insert(
                c.uri.clone(),
                CstCacheEntry {
                    parse_version: c.parse_version,
                    db_fingerprint: fingerprint,
                    diagnostics: c.diagnostics.clone(),
                },
            );
        }
    }

    let mut new_subscriptions: Vec<(String, ObservationSet)> = Vec::with_capacity(computed.len());
    for c in computed {
        if !c.diagnostics.is_empty() {
            out.insert(c.uri.clone(), c.diagnostics);
        }
        new_subscriptions.push((c.uri, c.observations));
    }

    CstDiagnosticsResult {
        by_uri: out,
        stats: CstCacheStats {
            hits,
            misses: miss_count,
        },
        new_subscriptions,
        cancelled,
    }
}

#[cfg(test)]
mod tests;
