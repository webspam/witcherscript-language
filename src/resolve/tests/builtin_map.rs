use super::super::{completion_members, hover_text, resolve_definition};
use crate::builtins::BUILTIN_MAP_URI;
use crate::formatter::ColonSpacing;
use crate::symbols::AccessLevel;
use crate::test_support::TestDb;
use crate::types::Type;

#[test]
fn map_member_params_substitute_key_type() {
    let t = TestDb::new("").with_builtins_index();
    let db = t.db();
    let def = db
        .find_member("map<name, CEntity>", "Contains", AccessLevel::Public)
        .expect("Contains should resolve on map<name, CEntity>");

    let params = db.display_parameters_of(&def);
    assert_eq!(
        params[0].type_annotation,
        Some(Type::from_annotation("name")),
        "key parameter must be substituted, not left as K"
    );
    assert_eq!(
        def.symbol.type_annotation,
        Some(Type::from_annotation("bool")),
        "Contains must return bool"
    );
}

#[test]
fn map_key_method_returns_key_type() {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_member("map<int, array<CEntity>>", "Key", AccessLevel::Public)
        .expect("Key should resolve on map<int, array<CEntity>>");

    assert_eq!(
        def.symbol.type_annotation,
        Some(Type::from_annotation("int")),
        "Key must return the key type"
    );
}

#[test]
fn map_method_container_name_becomes_generic_instance() {
    let t = TestDb::new("").with_builtins_index();
    let db = t.db();
    let def = db
        .find_member("map<int, float>", "Erase", AccessLevel::Public)
        .expect("Erase resolves");

    assert_eq!(
        def.symbol.container_name.as_deref(),
        Some("map<int, float>"),
        "container must become the generic instance"
    );
    let hover = hover_text(&def, &db, ColonSpacing::Spaced);
    assert!(
        hover.contains("map<int, float>.Erase"),
        "hover should show generic instance: {hover}"
    );
    assert!(hover.contains("key : int"), "hover: {hover}");
    assert_eq!(
        def.symbol.type_annotation,
        Some(Type::from_annotation("bool")),
        "Erase must return bool"
    );
}

#[test]
fn completion_after_dot_on_map_var_returns_methods() {
    let t = TestDb::new(concat!(
        "function Test() {\n",
        "  var m : map < name, bool >;\n",
        "  m.$0\n",
        "}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let db = t.db();
    let members = completion_members(&uri, t.doc_for(&uri), &db, pos);
    let mut names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    names.sort_unstable();

    assert_eq!(
        names,
        ["Clear", "Contains", "Erase", "Key", "Size"],
        "completion must list exactly the map methods"
    );
}

#[test]
fn goto_definition_on_map_method_resolves_into_builtin_file() {
    let t = TestDb::new(concat!(
        "function Test() {\n",
        "  var m : map<int, string>;\n",
        "  m.Cont$0ains(1);\n",
        "}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let def =
        resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("Contains should resolve");

    assert_eq!(
        def.uri, BUILTIN_MAP_URI,
        "definition must land in the builtin map file"
    );
    assert_eq!(
        def.symbol.name, "Contains",
        "definition must be the Contains method"
    );
}

#[test]
fn method_on_indexed_map_value_resolves_through_value_type() {
    let t = TestDb::new(concat!(
        "function Test() {\n",
        "  var m : map<int, array<string>>;\n",
        "  m[1].Push$0Back(\"x\");\n",
        "}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let def =
        resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("PushBack should resolve");

    assert_eq!(
        def.symbol.name, "PushBack",
        "definition must be the PushBack method"
    );
    assert_eq!(
        def.symbol.container_name.as_deref(),
        Some("array<string>"),
        "PushBack must resolve on the map's value type"
    );
}

#[test]
fn map_class_is_not_in_user_type_completions() {
    let t = TestDb::new("").with_builtins_index();
    let types = t.db().all_types();
    assert!(
        !types.iter().any(|d| d.symbol.name == "map"),
        "builtin map class should NOT appear in all_types()"
    );
}
