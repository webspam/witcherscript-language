use super::super::{completion_members, hover_text, parse_generic_type, resolve_definition};
use crate::builtins::BUILTIN_ARRAY_URI;
use crate::symbols::AccessLevel;
use crate::test_support::TestDb;

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
    let t = TestDb::new("").with_builtins_index();
    let db = t.db();

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
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_member("array<CEntity>", "Last", AccessLevel::Public)
        .expect("Last should resolve on array<CEntity>");

    assert_eq!(def.symbol.type_annotation.as_deref(), Some("CEntity"));
}

#[test]
fn array_method_with_concrete_param_type_is_unchanged() {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_member("array<CEntity>", "Resize", AccessLevel::Public)
        .expect("Resize resolves");

    let sig = def.symbol.signature.as_deref().unwrap_or("");
    assert!(sig.contains(": int"), "got: {sig}");
    assert_eq!(def.symbol.type_annotation.as_deref(), Some("void"));
}

#[test]
fn array_method_container_name_becomes_generic_instance() {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
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
    let t = TestDb::new("").with_builtins_index();
    let members = t.db().members_of_tiered("array<int>", AccessLevel::Public);
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
    let t = TestDb::new(concat!(
        "function Test() {\n",
        "  var xs : array<int>;\n",
        "  xs.$0\n",
        "}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let members = completion_members(&uri, t.doc_for(&uri), &t.db(), pos);

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
    let t = TestDb::new("").with_builtins_index();
    let types = t.db().all_types();
    assert!(
        !types.iter().any(|d| d.symbol.name == "array"),
        "builtin array class should NOT appear in all_types(); got {:?}",
        types.iter().map(|d| &d.symbol.name).collect::<Vec<_>>()
    );
}

#[test]
fn goto_definition_on_array_method_resolves_into_builtin_file() {
    let t = TestDb::new(concat!(
        "function Test() {\n",
        "  var xs : array<int>;\n",
        "  xs.Pus$0hBack(1);\n",
        "}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let def =
        resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("PushBack should resolve");

    assert_eq!(def.uri, BUILTIN_ARRAY_URI);
    assert_eq!(def.symbol.name, "PushBack");
}

#[test]
fn nested_array_substitutes_one_level() {
    let t = TestDb::new("").with_builtins_index();
    let db = t.db();

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
