use std::collections::HashMap;

use tower_lsp::lsp_types::Url;
use witcherscript_parser::diagnostics::{
    collect_cst_diagnostics_for_document, WorkspaceDiagnostic,
};
use witcherscript_parser::document::ParsedDocument;
use witcherscript_parser::resolve::SymbolDb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DbFingerprint {
    pub workspace: u64,
    pub base: u64,
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

pub(crate) fn cst_diagnostics_with_cache(
    documents: &HashMap<Url, ParsedDocument>,
    db: &SymbolDb,
    fingerprint: DbFingerprint,
    cache: &mut HashMap<Url, CstCacheEntry>,
) -> (HashMap<String, Vec<WorkspaceDiagnostic>>, CstCacheStats) {
    let mut out: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();
    let mut stats = CstCacheStats::default();

    for (url, document) in documents.iter() {
        let reuse = cache.get(url).is_some_and(|e| {
            e.parse_version == document.parse_version && e.db_fingerprint == fingerprint
        });
        let diagnostics = if reuse {
            stats.hits += 1;
            cache.get(url).unwrap().diagnostics.clone()
        } else {
            stats.misses += 1;
            let d = collect_cst_diagnostics_for_document(url.as_str(), document, db);
            cache.insert(
                url.clone(),
                CstCacheEntry {
                    parse_version: document.parse_version,
                    db_fingerprint: fingerprint,
                    diagnostics: d.clone(),
                },
            );
            d
        };
        if !diagnostics.is_empty() {
            out.insert(url.to_string(), diagnostics);
        }
    }

    cache.retain(|url, _| documents.contains_key(url));

    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::{cst_diagnostics_with_cache, CstCacheEntry, DbFingerprint};
    use std::collections::HashMap;
    use tower_lsp::lsp_types::Url;
    use witcherscript_parser::document::{parse_document, ParsedDocument};
    use witcherscript_parser::resolve::{SymbolDb, WorkspaceIndex};

    fn make_doc(src: &str) -> ParsedDocument {
        parse_document(src).expect("parse should succeed")
    }

    fn url(s: &str) -> Url {
        Url::parse(s).expect("valid url")
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

        let mut documents: HashMap<Url, ParsedDocument> = HashMap::new();
        documents.insert(url("file:///a.ws"), doc_a);
        documents.insert(url("file:///b.ws"), doc_b);
        let fp = DbFingerprint {
            workspace: idx.generation(),
            base: 0,
            env: 0,
        };

        let mut cache: HashMap<Url, CstCacheEntry> = HashMap::new();
        let (_, stats1) = cst_diagnostics_with_cache(&documents, &db, fp, &mut cache);
        assert_eq!(stats1.hits, 0);
        assert_eq!(stats1.misses, 2);

        let (_, stats2) = cst_diagnostics_with_cache(&documents, &db, fp, &mut cache);
        assert_eq!(stats2.hits, 2);
        assert_eq!(stats2.misses, 0);
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

        let mut documents: HashMap<Url, ParsedDocument> = HashMap::new();
        documents.insert(url("file:///a.ws"), doc_a);
        documents.insert(url("file:///b.ws"), doc_b);
        let fp = DbFingerprint {
            workspace: idx.generation(),
            base: 0,
            env: 0,
        };

        let mut cache: HashMap<Url, CstCacheEntry> = HashMap::new();
        let _ = cst_diagnostics_with_cache(&documents, &db, fp, &mut cache);

        let fresh_a = make_doc("class A {} // edit\n");
        documents.insert(url("file:///a.ws"), fresh_a);

        let (_, stats) = cst_diagnostics_with_cache(&documents, &db, fp, &mut cache);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
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

        let mut documents: HashMap<Url, ParsedDocument> = HashMap::new();
        documents.insert(url("file:///a.ws"), doc_a);
        documents.insert(url("file:///b.ws"), doc_b);
        let fp = DbFingerprint {
            workspace: idx.generation(),
            base: 0,
            env: 0,
        };

        let mut cache: HashMap<Url, CstCacheEntry> = HashMap::new();
        let _ = cst_diagnostics_with_cache(&documents, &db, fp, &mut cache);

        let fp_bumped = DbFingerprint {
            workspace: fp.workspace.wrapping_add(1),
            base: 0,
            env: 0,
        };
        let (_, stats) = cst_diagnostics_with_cache(&documents, &db, fp_bumped, &mut cache);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 2);
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

        let mut documents: HashMap<Url, ParsedDocument> = HashMap::new();
        documents.insert(url("file:///a.ws"), doc_a);
        documents.insert(url("file:///b.ws"), doc_b);
        let fp = DbFingerprint {
            workspace: idx.generation(),
            base: 0,
            env: 0,
        };

        let mut cache: HashMap<Url, CstCacheEntry> = HashMap::new();
        let _ = cst_diagnostics_with_cache(&documents, &db, fp, &mut cache);
        assert_eq!(cache.len(), 2);

        documents.remove(&url("file:///b.ws"));
        let _ = cst_diagnostics_with_cache(&documents, &db, fp, &mut cache);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key(&url("file:///a.ws")));
    }
}
