use crate::document::parse_document;
use crate::resolve::{
    SymbolDb, WorkspaceIndex, find_references, resolve_all_definitions, resolve_definition,
};
use crate::symbols::{AccessLevel, SymbolKind};
use crate::test_support::TestDb;

fn index(uri: &str, source: &str) -> WorkspaceIndex {
    let doc = parse_document(source).expect("parse");
    let mut idx = WorkspaceIndex::default();
    idx.update_document(uri, &doc);
    idx
}

fn resolve_at_cursor(fixture: &str) -> crate::resolve::Definition {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("cursor should resolve")
}

#[test]
fn state_resolves_by_class_name_not_short_name() {
    let workspace = index("file:///a.ws", "statemachine class C {}\nstate S in C {}\n");
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    let def = db.find_top_level("CStateS").expect("class name resolves");
    assert_eq!(
        def.symbol.kind,
        SymbolKind::State,
        "class name resolves to the state"
    );
    assert_eq!(def.symbol.name, "S", "state keeps its short name");
    assert_eq!(
        def.symbol.owner_class.as_deref(),
        Some("C"),
        "state records its owner"
    );
    assert!(db.find_top_level("S").is_none(), "short name is not a type");
}

#[test]
fn state_members_are_keyed_by_class_name() {
    let workspace = index(
        "file:///a.ws",
        "statemachine class C {}\nstate S in C { function Run() {} }\n",
    );
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    let run = db
        .find_member("CStateS", "Run", AccessLevel::Private)
        .expect("member found under the class name");
    assert_eq!(
        run.symbol.container_name.as_deref(),
        Some("CStateS"),
        "state members record the class name as their container"
    );
}

#[test]
fn same_short_name_in_two_owners_are_distinct_classes() {
    let workspace = index(
        "file:///a.ws",
        "statemachine class A {}\nstatemachine class B {}\n\
         state Shared in A { function OnA() {} }\nstate Shared in B { function OnB() {} }\n",
    );
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    assert!(
        db.find_member("AStateShared", "OnA", AccessLevel::Private)
            .is_some(),
        "A's Shared state should have OnA"
    );
    assert!(
        db.find_member("AStateShared", "OnB", AccessLevel::Private)
            .is_none(),
        "A's Shared state should not have B's OnB"
    );
    assert!(
        db.find_member("BStateShared", "OnB", AccessLevel::Private)
            .is_some(),
        "B's Shared state should have OnB"
    );
    assert!(
        db.find_member("BStateShared", "OnA", AccessLevel::Private)
            .is_none(),
        "B's Shared state should not have A's OnA"
    );
}

#[test]
fn state_superclass_is_base_state_class_name_resolved_through_owner_chain() {
    let workspace = index(
        "file:///a.ws",
        "statemachine class Player {}\nstatemachine class Witcher extends Player {}\n\
         state CombatSword in Player { function Swing() {} }\n\
         state CombatSteel in Witcher extends CombatSword {}\n",
    );
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    assert_eq!(
        db.superclass_of("WitcherStateCombatSteel").as_deref(),
        Some("PlayerStateCombatSword"),
        "the base state is found on the owner's ancestor and named by its class name",
    );
    assert!(
        db.find_member("WitcherStateCombatSteel", "Swing", AccessLevel::Public)
            .is_some(),
        "inherited from the base state found on the owner's ancestor",
    );
}

#[test]
fn state_without_extends_derives_from_scriptable_state() {
    let workspace = index("file:///a.ws", "statemachine class C {}\nstate S in C {}\n");
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    assert_eq!(
        db.superclass_of("CStateS").as_deref(),
        Some("CScriptableState"),
        "a state without extends derives from CScriptableState"
    );
}

#[test]
fn workspace_state_shadows_base_state_with_same_class_name() {
    let base = index(
        "file:///base.ws",
        "statemachine class C {}\nstate S in C {}\nstate Only in C {}\n",
    );
    let workspace = index(
        "file:///mod.ws",
        "statemachine class C {}\nstate S in C extends Only {}\n",
    );
    let db = SymbolDb::new(&workspace, &base);

    assert_eq!(
        db.find_top_level("CStateS").expect("workspace wins").uri,
        "file:///mod.ws",
        "the workspace state shadows the base state with the same class name"
    );
    assert_eq!(
        db.find_top_level("CStateOnly")
            .expect("base-only resolves")
            .uri,
        "file:///base.ws",
        "a state only in base still resolves from base"
    );
}

#[test]
fn removing_one_of_two_duplicate_declarations_keeps_the_class_name() {
    let doc = parse_document("statemachine class C {}\nstate S in C {}\n").expect("parse");
    let mut idx = WorkspaceIndex::default();
    idx.update_document("file:///a.ws", &doc);
    idx.update_document("file:///b.ws", &doc);

    idx.remove_document("file:///a.ws");
    assert!(
        idx.find_top_level("CStateS").is_some(),
        "still declared in b.ws"
    );

    idx.remove_document("file:///b.ws");
    assert!(
        idx.find_top_level("CStateS").is_none(),
        "no declaration left"
    );
}

#[test]
fn class_name_in_type_position_resolves_to_state_declaration() {
    let def = resolve_at_cursor(
        "statemachine class C {}\nstate Sleep in C {}\nfunction F() { var s : $0CStateSleep; }\n",
    );
    assert_eq!(
        def.symbol.kind,
        SymbolKind::State,
        "type-position class name resolves to a state"
    );
    assert_eq!(
        def.symbol.name, "Sleep",
        "type-position class name resolves to the Sleep state"
    );
}

#[test]
fn class_name_in_annotation_resolves_to_state_declaration() {
    let def = resolve_at_cursor(
        "statemachine class C {}\nstate Sleep in C {}\n@addMethod($0CStateSleep) function Extra() {}\n",
    );
    assert_eq!(
        def.symbol.kind,
        SymbolKind::State,
        "annotation class name resolves to a state"
    );
    assert_eq!(
        def.symbol.name, "Sleep",
        "annotation class name resolves to the Sleep state"
    );
}

#[test]
fn member_access_through_class_name_typed_receiver_resolves() {
    let def = resolve_at_cursor(
        "statemachine class C {}\nstate Sleep in C { function Doze() {} }\n\
         function F(s : CStateSleep) { s.$0Doze(); }\n",
    );
    assert_eq!(
        def.symbol.kind,
        SymbolKind::Method,
        "member access resolves to a method"
    );
    assert_eq!(
        def.symbol.name, "Doze",
        "a class-name typed receiver exposes the state's methods"
    );
}

#[test]
fn wrapped_method_in_class_name_wrapper_resolves_to_state_method() {
    let def = resolve_at_cursor(
        "statemachine class C {}\nstate Sleep in C { function Run() : int { return 1; } }\n\
         @wrapMethod(CStateSleep) function Run() : int { return $0wrappedMethod() + 1; }\n",
    );
    assert_eq!(
        def.symbol.kind,
        SymbolKind::Method,
        "wrappedMethod resolves to a method"
    );
    assert_eq!(
        def.symbol.container_name.as_deref(),
        Some("CStateSleep"),
        "wrappedMethod resolves to the wrapped state method"
    );
}

#[test]
fn parent_in_class_name_annotation_resolves_to_owner_member() {
    let def = resolve_at_cursor(
        "statemachine class C { function OwnerOnly() {} }\nstate Sleep in C {}\n\
         @addMethod(CStateSleep) function CallOwner() { parent.$0OwnerOnly(); }\n",
    );
    assert_eq!(
        def.symbol.container_name.as_deref(),
        Some("C"),
        "parent inside a class-name annotation is the owner class"
    );
}

#[test]
fn this_in_class_name_annotation_resolves_to_state_member() {
    let def = resolve_at_cursor(
        "statemachine class C {}\nstate Sleep in C { var secret : int; }\n\
         @addMethod(CStateSleep) function Read() : int { return this.$0secret; }\n",
    );
    assert_eq!(
        def.symbol.kind,
        SymbolKind::Field,
        "this.secret resolves to a field"
    );
    assert_eq!(
        def.symbol.container_name.as_deref(),
        Some("CStateSleep"),
        "this inside a class-name annotation is the state"
    );
}

#[test]
fn class_name_added_method_resolves_from_state_body() {
    let def = resolve_at_cursor(
        "statemachine class C {}\nstate Sleep in C { function Use() { $0Added(); } }\n\
         @addMethod(CStateSleep) function Added() {}\n",
    );
    assert_eq!(
        def.symbol.kind,
        SymbolKind::Function,
        "the added method is an annotated function"
    );
    assert_eq!(
        def.symbol.name, "Added",
        "a bare call in the state body reaches the method added via the class name"
    );
}

#[test]
fn super_in_state_resolves_to_base_state() {
    let def = resolve_at_cursor(
        "statemachine class C {}\nstate Base in C { function Ping() {} }\n\
         state Child in C extends Base { function Ping() { $0super.Ping(); } }\n",
    );
    assert_eq!(
        def.symbol.kind,
        SymbolKind::State,
        "super inside a state resolves to a state"
    );
    assert_eq!(
        def.symbol.name, "Base",
        "super resolves to the extended base state"
    );
}

#[test]
fn references_to_state_include_class_name_usages() {
    let t = TestDb::new(
        "statemachine class C {}\nstate $0Sleep in C {}\n\
         function F(s : CStateSleep) {}\n@addMethod(CStateSleep) function Extra() {}\n",
    );
    let (uri, pos) = t.cursor();
    let db = t.db();
    let def = resolve_definition(&uri, t.doc_for(&uri), &db, pos).expect("state declaration");
    let refs = find_references(&def, t.doc_for(&uri), &t.search_docs(), &db, true);
    assert_eq!(
        refs.len(),
        3,
        "declaration plus two class-name usages: {refs:?}"
    );
}

#[test]
fn references_to_state_report_the_short_name_span() {
    let t = TestDb::new(
        "statemachine class C {}\nstate $0Sleep in C {}\n\
         function F(s : CStateSleep) {}\n",
    );
    let (uri, pos) = t.cursor();
    let db = t.db();
    let def = resolve_definition(&uri, t.doc_for(&uri), &db, pos).expect("state declaration");
    let refs = find_references(&def, t.doc_for(&uri), &t.search_docs(), &db, true);
    let spans: Vec<(u32, u32, u32)> = refs
        .iter()
        .map(|r| {
            (
                r.range.start.character,
                r.name_range.start.character,
                r.name_range.end.character,
            )
        })
        .collect();
    assert_eq!(
        spans,
        [(6, 6, 11), (15, 21, 26)],
        "the declaration's name span is the whole ident; the class-name usage's name span skips `CState`"
    );
}

// OwnerPing lives only on the owner class, so this resolves only if virtualParent targets the
// owner (not the state's base): https://github.com/webspam/witcherscript-language/issues/114
#[test]
fn virtual_parent_member_resolves_against_the_owner_class() {
    let def = resolve_at_cursor(
        "class Owner {\n    function OwnerPing() : int { return 1; }\n}\nstate S in Owner {\n    function M() {\n        virtual_parent.$0OwnerPing();\n    }\n}\n",
    );
    assert_eq!(
        def.symbol.kind,
        SymbolKind::Method,
        "virtualParent resolves a method"
    );
    assert_eq!(
        def.symbol.name, "OwnerPing",
        "virtualParent resolves a member of the owner class"
    );
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
