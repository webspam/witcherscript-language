use crate::document::parse_document;
use crate::resolve::{SymbolDb, WorkspaceIndex, resolve_definition};
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

fn index(uri: &str, source: &str) -> WorkspaceIndex {
    let doc = parse_document(source).expect("parse");
    let mut idx = WorkspaceIndex::default();
    idx.update_document(uri, &doc);
    idx
}

#[test]
fn resolves_synthetic_name_to_class_extending_state() {
    let idx = index("file:///a.ws", "statemachine class C {}\nstate S in C {}\n");
    let backing = idx
        .find_state_backing_class("CStateS")
        .expect("backing class for state S in C");
    assert_eq!(backing.state_name(), "S");
    let def = backing.as_class_definition();
    assert_eq!(def.symbol.name, "CStateS");
    assert_eq!(def.symbol.kind, SymbolKind::Class);
    assert_eq!(
        def.symbol.base_class.as_deref(),
        Some("S"),
        "the backing class extends its state"
    );
}

#[test]
fn returns_none_for_unknown_name() {
    let idx = index("file:///a.ws", "statemachine class C {}\nstate S in C {}\n");
    assert!(idx.find_state_backing_class("CStateMissing").is_none());
    assert!(idx.find_state_backing_class("S").is_none());
}

#[test]
fn duplicate_state_across_files_survives_one_removal() {
    let doc = parse_document("statemachine class C {}\nstate S in C {}\n").expect("parse");
    let mut idx = WorkspaceIndex::default();
    idx.update_document("file:///a.ws", &doc);
    idx.update_document("file:///b.ws", &doc);

    idx.remove_document("file:///a.ws");
    assert!(
        idx.find_state_backing_class("CStateS").is_some(),
        "still declared in b.ws"
    );

    idx.remove_document("file:///b.ws");
    assert!(
        idx.find_state_backing_class("CStateS").is_none(),
        "no declaration left"
    );
}

#[test]
fn symbol_db_prefers_workspace_then_falls_back_to_base() {
    let base = index(
        "file:///base.ws",
        "statemachine class C {}\nstate S in C {}\nstate Only in C {}\n",
    );
    let workspace = index(
        "file:///mod.ws",
        "statemachine class C {}\nstate S in C extends Only {}\n",
    );
    let db = SymbolDb::new(&workspace, &base);

    let shadowed = db
        .find_state_backing_class("CStateS")
        .expect("workspace wins");
    assert_eq!(
        shadowed.as_class_definition().uri,
        "file:///mod.ws",
        "workspace state shadows the base-only one"
    );

    let base_only = db
        .find_state_backing_class("CStateOnly")
        .expect("base-only backing class still resolves");
    assert_eq!(base_only.state_name(), "Only");
}

#[test]
fn synthetic_class_resolves_by_name_but_stays_out_of_type_completions() {
    let workspace = index(
        "file:///a.ws",
        "statemachine class C {}\nstate Sleep in C {}\n",
    );
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    assert!(
        db.find_top_level("CStateSleep").is_some(),
        "resolvable by name"
    );
    assert!(
        db.all_types()
            .iter()
            .all(|d| d.symbol.name != "CStateSleep"),
        "synthetic class must not appear in the type-completion catalog"
    );
}

#[test]
fn synthetic_type_name_resolves_as_class_extending_the_state() {
    let t = TestDb::new(
        "statemachine class C {}\nstate Sleep in C {}\nfunction F() { var s : $0CStateSleep; }\n",
    );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("CStateSleep resolves as a known type");
    assert_eq!(def.symbol.kind, SymbolKind::Class);
    assert_eq!(def.symbol.name, "CStateSleep");
    assert_eq!(
        def.symbol.base_class.as_deref(),
        Some("Sleep"),
        "the backing class extends its state"
    );
}

#[test]
fn member_access_through_synthetic_type_resolves() {
    let t = TestDb::new(
        "statemachine class C {}\nstate Sleep in C { function Doze() {} }\nfunction F() { var s : CStateSleep; s.$0Doze(); }\n",
    );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("member of a synthetic-state-typed receiver resolves");
    assert_eq!(def.symbol.kind, SymbolKind::Method);
    assert_eq!(def.symbol.name, "Doze");
}

// OwnerPing lives only on the owner class, so this resolves only if virtualParent targets the
// owner (not the state's base): https://github.com/webspam/witcherscript-language/issues/114
#[test]
fn virtual_parent_member_resolves_against_the_owner_class() {
    let t = TestDb::new(
        "class Owner {\n    function OwnerPing() : int { return 1; }\n}\nstate S in Owner {\n    function M() {\n        virtual_parent.$0OwnerPing();\n    }\n}\n",
    );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("virtualParent resolves a member of the owner class");
    assert_eq!(def.symbol.kind, SymbolKind::Method);
    assert_eq!(def.symbol.name, "OwnerPing");
}

fn owning_classes(defs: &[crate::resolve::Definition]) -> Vec<&str> {
    let mut classes: Vec<&str> = defs
        .iter()
        .map(|d| d.symbol.container_name.as_deref().unwrap_or(""))
        .collect();
    classes.sort_unstable();
    classes
}

// virtualParent dispatches to the runtime class, so go-to-definition must list both the owner's
// method and every subclass override: https://github.com/webspam/witcherscript-language/issues/114
#[test]
fn virtual_parent_goto_lists_owner_and_subclass_overrides() {
    use crate::resolve::resolve_all_definitions;
    let t = TestDb::new(
        "class BaseClass {\n    function IsAPotato() : bool { return false; }\n}\nclass SomeClass extends BaseClass {\n    function IsAPotato() : bool { return true; }\n}\nstate Potato in BaseClass {\n    function M() {\n        virtual_parent.$0IsAPotato();\n    }\n}\n",
    );
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(
        owning_classes(&defs),
        ["BaseClass", "SomeClass"],
        "virtualParent go-to-def should reach the owner and its subclass override"
    );
}

// A subclass two levels below the owner is still a possible runtime type.
#[test]
fn virtual_parent_goto_includes_transitive_subclass_override() {
    use crate::resolve::resolve_all_definitions;
    let t = TestDb::new(
        "class A {\n    function Ping() {}\n}\nclass B extends A {}\nclass C extends B {\n    function Ping() {}\n}\nstate S in A {\n    function M() {\n        virtual_parent.$0Ping();\n    }\n}\n",
    );
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(
        owning_classes(&defs),
        ["A", "C"],
        "the override two levels down must be listed; the non-overriding middle class must not"
    );
}

// `parent` is static, so it must stay a single owner target even when subclasses override.
#[test]
fn parent_goto_does_not_list_subclass_overrides() {
    use crate::resolve::resolve_all_definitions;
    let t = TestDb::new(
        "class BaseClass {\n    function IsAPotato() : bool { return false; }\n}\nclass SomeClass extends BaseClass {\n    function IsAPotato() : bool { return true; }\n}\nstate Potato in BaseClass {\n    function M() {\n        parent.$0IsAPotato();\n    }\n}\n",
    );
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(
        owning_classes(&defs),
        ["BaseClass"],
        "parent dispatches statically to the owner only"
    );
}
