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
mod tests {
    use super::{cst_diagnostics_with_cache, CstCacheEntry, DbFingerprint};
    use std::collections::HashMap;
    use witcherscript_language::document::{parse_document, ParsedDocument};
    use witcherscript_language::resolve::{SymbolDb, WorkspaceIndex};

    fn make_doc(src: &str) -> ParsedDocument {
        parse_document(src).expect("parse should succeed")
    }

    fn fp() -> DbFingerprint {
        DbFingerprint {
            base_surface: 0,
            env: 0,
        }
    }

    #[test]
    fn unchanged_docs_hit_cache_on_second_call() {
        let mut idx = WorkspaceIndex::default();
        let doc_a = make_doc("class A { function F() {} function T() { var a : A; a.F(); } }\n");
        let doc_b = make_doc("class B { function G() {} function T() { var b : B; b.G(); } }\n");
        idx.update_document("file:///a.ws", &doc_a);
        idx.update_document("file:///b.ws", &doc_b);
        let base = WorkspaceIndex::default();
        let db = SymbolDb::new(&idx, &base);

        let mut documents: HashMap<String, &ParsedDocument> = HashMap::new();
        documents.insert("file:///a.ws".to_string(), &doc_a);
        documents.insert("file:///b.ws".to_string(), &doc_b);

        let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
        let r1 = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);
        assert_eq!(r1.stats.hits, 0);
        assert_eq!(r1.stats.misses, 2);

        let r2 = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);
        assert_eq!(r2.stats.hits, 2);
        assert_eq!(r2.stats.misses, 0);
    }

    #[test]
    fn text_only_edit_to_doc_keeps_others_hot() {
        let mut idx = WorkspaceIndex::default();
        let doc_a = make_doc("class A {}\n");
        let doc_b = make_doc("class B {}\n");
        idx.update_document("file:///a.ws", &doc_a);
        idx.update_document("file:///b.ws", &doc_b);
        let base = WorkspaceIndex::default();

        let mut documents: HashMap<String, &ParsedDocument> = HashMap::new();
        documents.insert("file:///a.ws".to_string(), &doc_a);
        documents.insert("file:///b.ws".to_string(), &doc_b);

        let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
        {
            let db = SymbolDb::new(&idx, &base);
            let _ = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);
        }

        let fresh_a = make_doc("class A {} // comment-only edit\n");
        idx.update_document("file:///a.ws", &fresh_a);
        documents.insert("file:///a.ws".to_string(), &fresh_a);

        let db = SymbolDb::new(&idx, &base);
        let r = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);
        assert_eq!(r.stats.hits, 1, "doc b should still be a cache hit");
        assert_eq!(
            r.stats.misses, 1,
            "only doc a (parse_version changed) should miss"
        );
    }

    #[test]
    fn edited_doc_misses_others_hit() {
        let mut idx = WorkspaceIndex::default();
        let doc_a = make_doc("class A {}\n");
        let doc_b = make_doc("class B {}\n");
        idx.update_document("file:///a.ws", &doc_a);
        idx.update_document("file:///b.ws", &doc_b);
        let base = WorkspaceIndex::default();
        let db = SymbolDb::new(&idx, &base);

        let mut documents: HashMap<String, &ParsedDocument> = HashMap::new();
        documents.insert("file:///a.ws".to_string(), &doc_a);
        documents.insert("file:///b.ws".to_string(), &doc_b);

        let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
        let _ = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);

        let fresh_a = make_doc("class A {} // edit\n");
        documents.insert("file:///a.ws".to_string(), &fresh_a);

        let r = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);
        assert_eq!(r.stats.hits, 1);
        assert_eq!(r.stats.misses, 1);
    }

    #[test]
    fn fingerprint_change_invalidates_all() {
        let mut idx = WorkspaceIndex::default();
        let doc_a = make_doc("class A {}\n");
        let doc_b = make_doc("class B {}\n");
        idx.update_document("file:///a.ws", &doc_a);
        idx.update_document("file:///b.ws", &doc_b);
        let base = WorkspaceIndex::default();
        let db = SymbolDb::new(&idx, &base);

        let mut documents: HashMap<String, &ParsedDocument> = HashMap::new();
        documents.insert("file:///a.ws".to_string(), &doc_a);
        documents.insert("file:///b.ws".to_string(), &doc_b);

        let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
        let _ = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);

        let fp_bumped = DbFingerprint {
            base_surface: fp().base_surface.wrapping_add(1),
            env: 0,
        };
        let r = cst_diagnostics_with_cache(&documents, &db, None, fp_bumped, &mut cache);
        assert_eq!(r.stats.hits, 0);
        assert_eq!(r.stats.misses, 2);
    }

    #[test]
    fn closed_docs_evicted_from_cache() {
        let mut idx = WorkspaceIndex::default();
        let doc_a = make_doc("class A {}\n");
        let doc_b = make_doc("class B {}\n");
        idx.update_document("file:///a.ws", &doc_a);
        idx.update_document("file:///b.ws", &doc_b);
        let base = WorkspaceIndex::default();
        let db = SymbolDb::new(&idx, &base);

        let mut documents: HashMap<String, &ParsedDocument> = HashMap::new();
        documents.insert("file:///a.ws".to_string(), &doc_a);
        documents.insert("file:///b.ws".to_string(), &doc_b);

        let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
        let _ = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);
        assert_eq!(cache.len(), 2);

        documents.remove("file:///b.ws");
        let _ = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn editing_unobserved_doc_does_not_invalidate_dependents() {
        let mut idx = WorkspaceIndex::default();
        let helper_uri = "file:///helper.ws";
        let user_uri = "file:///user.ws";
        let helper = make_doc("function Log() {}\n");
        let user = make_doc("function F() { var x : int; x = 1; }\n");
        let _ = idx.update_document(helper_uri, &helper);
        let _ = idx.update_document(user_uri, &user);
        let base = WorkspaceIndex::default();

        let mut documents: HashMap<String, &ParsedDocument> = HashMap::new();
        documents.insert(user_uri.to_string(), &user);
        documents.insert(helper_uri.to_string(), &helper);
        let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();

        let warm = {
            let db = SymbolDb::new(&idx, &base);
            cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache)
        };
        for (uri, obs) in warm.new_subscriptions {
            idx.register_subscription(&uri, obs);
        }
        assert_eq!(warm.stats.misses, 2);

        let fresh_helper = make_doc("function Log() {} function Trace() {}\n");
        let invalidated = idx.update_document(helper_uri, &fresh_helper);
        documents.insert(helper_uri.to_string(), &fresh_helper);
        cache.retain(|u, _| !invalidated.contains(u.as_str()));

        let stats = {
            let db = SymbolDb::new(&idx, &base);
            cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache).stats
        };
        assert!(
            !invalidated.contains(user_uri),
            "user.ws never observed helper.ws's symbols and must not be invalidated; got {invalidated:?}"
        );
        assert_eq!(stats.hits, 1, "user.ws should still be a hit");
        assert_eq!(
            stats.misses, 1,
            "helper.ws should miss (its own parse_version changed)"
        );
    }
}
