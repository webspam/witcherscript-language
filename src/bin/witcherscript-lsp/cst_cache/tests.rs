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
        legacy_db_generation: 0,
    }
}

fn two_doc_fixture(src_a: &str, src_b: &str) -> (WorkspaceIndex, ParsedDocument, ParsedDocument) {
    let doc_a = make_doc(src_a);
    let doc_b = make_doc(src_b);
    let mut idx = WorkspaceIndex::default();
    idx.update_document("file:///a.ws", &doc_a);
    idx.update_document("file:///b.ws", &doc_b);
    (idx, doc_a, doc_b)
}

fn docs_map<'a>(
    a: &'a ParsedDocument,
    b: &'a ParsedDocument,
) -> HashMap<String, &'a ParsedDocument> {
    let mut documents = HashMap::new();
    documents.insert("file:///a.ws".to_string(), a);
    documents.insert("file:///b.ws".to_string(), b);
    documents
}

#[test]
fn unchanged_docs_hit_cache_on_second_call() {
    let (idx, doc_a, doc_b) = two_doc_fixture(
        "class A { function F() {} function T() { var a : A; a.F(); } }\n",
        "class B { function G() {} function T() { var b : B; b.G(); } }\n",
    );
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&idx, &base);
    let documents = docs_map(&doc_a, &doc_b);

    let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
    let r1 = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true);
    assert_eq!(r1.stats.hits, 0);
    assert_eq!(r1.stats.misses, 2);

    let r2 = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true);
    assert_eq!(r2.stats.hits, 2);
    assert_eq!(r2.stats.misses, 0);
}

#[test]
fn text_only_edit_to_doc_keeps_others_hot() {
    let (mut idx, doc_a, doc_b) = two_doc_fixture("class A {}\n", "class B {}\n");
    let base = WorkspaceIndex::default();
    let mut documents = docs_map(&doc_a, &doc_b);

    let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
    {
        let db = SymbolDb::new(&idx, &base);
        let _ = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true);
    }

    let fresh_a = make_doc("class A {} // comment-only edit\n");
    idx.update_document("file:///a.ws", &fresh_a);
    documents.insert("file:///a.ws".to_string(), &fresh_a);

    let db = SymbolDb::new(&idx, &base);
    let r = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true);
    assert_eq!(r.stats.hits, 1, "doc b should still be a cache hit");
    assert_eq!(
        r.stats.misses, 1,
        "only doc a (parse_version changed) should miss"
    );
}

#[test]
fn edited_doc_misses_others_hit() {
    let (idx, doc_a, doc_b) = two_doc_fixture("class A {}\n", "class B {}\n");
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&idx, &base);
    let mut documents = docs_map(&doc_a, &doc_b);

    let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
    let _ = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true);

    let fresh_a = make_doc("class A {} // edit\n");
    documents.insert("file:///a.ws".to_string(), &fresh_a);

    let r = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true);
    assert_eq!(r.stats.hits, 1);
    assert_eq!(r.stats.misses, 1);
}

#[test]
fn fingerprint_change_invalidates_all() {
    let (idx, doc_a, doc_b) = two_doc_fixture("class A {}\n", "class B {}\n");
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&idx, &base);
    let documents = docs_map(&doc_a, &doc_b);

    let mut cache: HashMap<String, CstCacheEntry> = HashMap::new();
    let _ = cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true);

    let fp_bumped = DbFingerprint {
        base_surface: fp().base_surface.wrapping_add(1),
        env: 0,
        legacy_db_generation: 0,
    };
    let r = cst_diagnostics_with_cache(&documents, &db, None, fp_bumped, &mut cache, &|| true);
    assert_eq!(r.stats.hits, 0);
    assert_eq!(r.stats.misses, 2);
}

#[test]
fn editing_unobserved_doc_does_not_invalidate_dependents() {
    let mut idx = WorkspaceIndex::default();
    let mut subscriptions = witcherscript_language::resolve::SubscriptionRegistry::default();
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
        cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true)
    };
    for (uri, obs) in warm.new_subscriptions {
        subscriptions.register(&uri, obs);
    }
    assert_eq!(warm.stats.misses, 2);

    let fresh_helper = make_doc("function Log() {} function Trace() {}\n");
    let changed = idx.update_document(helper_uri, &fresh_helper);
    let invalidated = subscriptions.subscribers_of(&changed);
    documents.insert(helper_uri.to_string(), &fresh_helper);
    cache.retain(|u, _| !invalidated.contains(u.as_str()));

    let stats = {
        let db = SymbolDb::new(&idx, &base);
        cst_diagnostics_with_cache(&documents, &db, None, fp(), &mut cache, &|| true).stats
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
