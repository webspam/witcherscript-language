use crate::document::parse_document;
use crate::resolve::{SymbolDb, WorkspaceIndex};

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
