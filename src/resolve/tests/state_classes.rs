use crate::document::parse_document;
use crate::resolve::{resolve_definition, SymbolDb, WorkspaceIndex};
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

fn index(uri: &str, source: &str) -> WorkspaceIndex {
    let doc = parse_document(source).expect("parse");
    let mut idx = WorkspaceIndex::default();
    idx.update_document(uri, &doc);
    idx
}

#[test]
fn resolves_synthetic_name_to_owner_and_state() {
    let idx = index("file:///a.ws", "statemachine class C {}\nstate S in C {}\n");
    let backing = idx
        .find_state_backing_class("CStateS")
        .expect("backing class for state S in C");
    assert_eq!(backing.name(), "CStateS");
    assert_eq!(backing.owner_class(), "C");
    assert_eq!(backing.state_name(), "S");
    assert_eq!(backing.base_class(), None, "no extends => no stored base");
}

#[test]
fn base_class_reflects_extends_clause() {
    let idx = index(
        "file:///a.ws",
        "statemachine class C {}\nstate BaseS in C {}\nstate S in C extends BaseS {}\n",
    );
    let backing = idx.find_state_backing_class("CStateS").expect("backing");
    assert_eq!(backing.base_class(), Some("BaseS"));
}

#[test]
fn returns_none_for_unknown_name() {
    let idx = index("file:///a.ws", "statemachine class C {}\nstate S in C {}\n");
    assert!(idx.find_state_backing_class("CStateMissing").is_none());
    assert!(idx.find_state_backing_class("S").is_none());
}

#[test]
fn iterator_yields_every_backing_class() {
    let idx = index(
        "file:///a.ws",
        "statemachine class C {}\nstate A in C {}\nstate B in C {}\n",
    );
    let mut names: Vec<String> = idx
        .state_backing_classes()
        .map(|b| b.name().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["CStateA".to_string(), "CStateB".to_string()]);
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
        shadowed.base_class(),
        Some("Only"),
        "workspace state shadows the base-only one"
    );

    let base_only = db
        .find_state_backing_class("CStateOnly")
        .expect("base-only backing class still resolves");
    assert_eq!(base_only.state_name(), "Only");
}

#[test]
fn synthetic_class_resolves_by_name_but_stays_out_of_type_completions() {
    let workspace = index("file:///a.ws", "statemachine class C {}\nstate Sleep in C {}\n");
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    assert!(
        db.find_top_level("CStateSleep").is_some(),
        "resolvable by name"
    );
    assert!(
        db.all_types().iter().all(|d| d.symbol.name != "CStateSleep"),
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
