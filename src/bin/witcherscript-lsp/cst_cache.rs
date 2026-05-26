use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

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
}

pub(crate) fn cst_diagnostics_with_cache(
    documents: &HashMap<String, &ParsedDocument>,
    db: &SymbolDb,
    loose: Option<(&SymbolDb, &HashSet<String>)>,
    fingerprint: DbFingerprint,
    cache: &mut HashMap<String, CstCacheEntry>,
) -> CstDiagnosticsResult {
    let mut out: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();
    let mut stats = CstCacheStats::default();
    let mut new_subscriptions: Vec<(String, ObservationSet)> = Vec::new();

    for (uri, document) in documents.iter() {
        let reuse = cache.get(uri).is_some_and(|e| {
            e.parse_version == document.parse_version && e.db_fingerprint == fingerprint
        });
        let diagnostics = if reuse {
            stats.hits += 1;
            cache.get(uri).unwrap().diagnostics.clone()
        } else {
            stats.misses += 1;
            let observations = Mutex::new(ObservationSet::default());
            let doc_db = match loose {
                Some((loose_db, loose_uris)) if loose_uris.contains(uri.as_str()) => loose_db,
                _ => db,
            };
            let recording_db = doc_db.with_observer(&observations);
            let d =
                tracing::debug_span!("cst_doc", uri = uri.as_str(), bytes = document.source.len())
                    .in_scope(|| {
                        collect_cst_diagnostics_for_document(uri.as_str(), document, &recording_db)
                    });
            cache.insert(
                uri.clone(),
                CstCacheEntry {
                    parse_version: document.parse_version,
                    db_fingerprint: fingerprint,
                    diagnostics: d.clone(),
                },
            );
            new_subscriptions.push((uri.clone(), observations.into_inner().unwrap()));
            d
        };
        if !diagnostics.is_empty() {
            out.insert(uri.clone(), diagnostics);
        }
    }

    cache.retain(|uri, _| documents.contains_key(uri));

    CstDiagnosticsResult {
        by_uri: out,
        stats,
        new_subscriptions,
    }
}

#[cfg(test)]
mod tests;
