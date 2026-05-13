use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::symbols::AccessLevel;

#[test]
fn parameters_of_returns_names_in_source_order() {
    let doc = make_doc(
        "function Find(findName : string, range : float, shouldScanAllObjects : bool) : int {}",
    );
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db.find_top_level("Find").expect("Find should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["findName", "range", "shouldScanAllObjects"]);
}

#[test]
fn parameters_of_returns_empty_for_zero_param_function() {
    let doc = make_doc("function NoArgs() {}");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db
        .find_top_level("NoArgs")
        .expect("NoArgs should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert!(params.is_empty());
}

#[test]
fn parameters_of_works_for_class_method() {
    let doc = make_doc("class CPlayer { function GetHealth(modifier : float) : int {} }");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db
        .find_member("CPlayer", "GetHealth", AccessLevel::Public)
        .expect("GetHealth should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["modifier"]);
}

#[test]
fn parameters_of_works_for_event() {
    let doc = make_doc("class C { event OnSpawn(spawnData : int) {} }");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db
        .find_member("C", "OnSpawn", AccessLevel::Public)
        .expect("OnSpawn should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["spawnData"]);
}

#[test]
fn parameters_of_skips_optional_params() {
    let doc = make_doc("function Find(name : string, optional range : float) : int {}");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db.find_top_level("Find").expect("Find should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["name"]);
}

#[test]
fn parameters_of_multi_name_group() {
    let doc = make_doc("function Multi(a, b : int, c : string) {}");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db.find_top_level("Multi").expect("Multi should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["a", "b", "c"]);
}
