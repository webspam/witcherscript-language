use super::super::{
    completion_members, hover_text, parse_generic_type, resolve_definition, SymbolDb,
    WorkspaceIndex,
};
use super::{make_doc, make_index};
use crate::builtins::{load_builtins_index, BUILTIN_ARRAY_URI};
use crate::line_index::SourcePosition;
use crate::symbols::AccessLevel;

fn builtins_db<'a>(
    workspace: &'a WorkspaceIndex,
    base: &'a WorkspaceIndex,
    builtins: &'a WorkspaceIndex,
) -> SymbolDb<'a> {
    SymbolDb::new(workspace, base).with_builtins(builtins)
}

#[test]
fn parse_generic_type_handles_basic_and_nested() {
    assert_eq!(parse_generic_type("array<int>"), Some(("array", "int")));
    assert_eq!(
        parse_generic_type("array<CEntity>"),
        Some(("array", "CEntity"))
    );
    assert_eq!(
        parse_generic_type("array<array<int>>"),
        Some(("array", "array<int>"))
    );
    assert_eq!(parse_generic_type("Foo"), None);
    assert_eq!(parse_generic_type("array<>"), None);
}

#[test]
fn array_int_member_is_resolved_with_substituted_param_type() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let def = db
        .find_member("array<int>", "PushBack", AccessLevel::Public)
        .expect("PushBack should resolve on array<int>");

    assert_eq!(def.symbol.name, "PushBack");
    let sig = def.symbol.signature.as_deref().unwrap_or("");
    assert!(sig.contains(": int"), "got signature: {sig}");
    assert!(
        !sig.contains(": T"),
        "signature should not still contain T: {sig}"
    );
}

#[test]
fn array_method_returning_placeholder_substitutes_return_type() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let def = db
        .find_member("array<CEntity>", "Last", AccessLevel::Public)
        .expect("Last should resolve on array<CEntity>");

    assert_eq!(def.symbol.type_annotation.as_deref(), Some("CEntity"));
}

#[test]
fn array_method_with_concrete_param_type_is_unchanged() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let def = db
        .find_member("array<CEntity>", "Resize", AccessLevel::Public)
        .expect("Resize resolves");

    let sig = def.symbol.signature.as_deref().unwrap_or("");
    assert!(sig.contains(": int"), "got: {sig}");
    assert_eq!(def.symbol.type_annotation.as_deref(), Some("void"));
}

#[test]
fn array_method_container_name_becomes_generic_instance() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let def = db
        .find_member("array<int>", "PushBack", AccessLevel::Public)
        .expect("PushBack resolves");

    assert_eq!(def.symbol.container_name.as_deref(), Some("array<int>"));
    let hover = hover_text(&def);
    assert!(
        hover.contains("array<int>.PushBack"),
        "hover should show generic instance: {hover}"
    );
    assert!(hover.contains(": int"), "hover: {hover}");
}

#[test]
fn members_of_array_int_lists_all_methods_substituted() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let members = db.members_of_tiered("array<int>", AccessLevel::Public);
    let names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    for expected in [
        "Clear",
        "Contains",
        "Erase",
        "EraseFast",
        "FindFirst",
        "Grow",
        "Insert",
        "Last",
        "PopBack",
        "PushBack",
        "Remove",
        "Resize",
        "Size",
    ] {
        assert!(names.contains(&expected), "missing {expected} in {names:?}");
    }

    let push_back = members
        .iter()
        .find(|(_, d)| d.symbol.name == "PushBack")
        .expect("PushBack present");
    let sig = push_back.1.symbol.signature.as_deref().unwrap_or("");
    assert!(sig.contains(": int"), "got: {sig}");
}

#[test]
fn completion_after_dot_on_array_var_returns_methods() {
    let source = concat!(
        "function Test() {\n",
        "  var xs : array<int>;\n",
        "  xs.\n",
        "}\n",
    );
    let doc = make_doc(source);
    let workspace = make_index("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let builtins = load_builtins_index();
    let db = builtins_db(&workspace, &base, &builtins);

    let members = completion_members(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 5,
        },
    );

    let names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    assert!(names.contains(&"PushBack"), "got: {names:?}");
    assert!(names.contains(&"Size"), "got: {names:?}");

    let push_back = members
        .iter()
        .find(|(_, d)| d.symbol.name == "PushBack")
        .unwrap();
    let sig = push_back.1.symbol.signature.as_deref().unwrap_or("");
    assert!(sig.contains(": int"), "got: {sig}");
}

#[test]
fn array_class_is_not_in_user_type_completions() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let types = db.all_types();
    assert!(
        !types.iter().any(|d| d.symbol.name == "array"),
        "builtin array class should NOT appear in all_types(); got {:?}",
        types.iter().map(|d| &d.symbol.name).collect::<Vec<_>>()
    );
}

#[test]
fn goto_definition_on_array_method_resolves_into_builtin_file() {
    let source = concat!(
        "function Test() {\n",
        "  var xs : array<int>;\n",
        "  xs.PushBack(1);\n",
        "}\n",
    );
    let doc = make_doc(source);
    let workspace = make_index("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let builtins = load_builtins_index();
    let db = builtins_db(&workspace, &empty, &builtins);

    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 8,
        },
    )
    .expect("PushBack should resolve");

    assert_eq!(def.uri, BUILTIN_ARRAY_URI);
    assert_eq!(def.symbol.name, "PushBack");
}

#[test]
fn nested_array_substitutes_one_level() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let def = db
        .find_member("array<array<int>>", "Last", AccessLevel::Public)
        .expect("Last on array<array<int>>");
    assert_eq!(def.symbol.type_annotation.as_deref(), Some("array<int>"));

    let push = db
        .find_member("array<array<int>>", "PushBack", AccessLevel::Public)
        .expect("PushBack on array<array<int>>");
    let sig = push.symbol.signature.as_deref().unwrap_or("");
    assert!(sig.contains(": array<int>"), "got: {sig}");
}
