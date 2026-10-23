use super::*;
use crate::symbols::{AccessLevel, SymbolKind};
use crate::types::Type;

#[test]
fn array_class_is_indexed() {
    let index = load_builtins_index();
    let def = index.find_top_level("array").expect("array class indexed");
    assert_eq!(def.symbol.kind, SymbolKind::Class);
    assert_eq!(def.symbol.origin, SymbolOrigin::Builtin);
    assert_eq!(def.uri, BUILTIN_ARRAY_URI);
}

#[test]
fn array_members_are_indexed_with_placeholder_types() {
    let index = load_builtins_index();
    let push_back = index
        .direct_member_of("array", "PushBack", AccessLevel::Public)
        .expect("PushBack indexed");
    assert_eq!(push_back.symbol.kind, SymbolKind::Method);
    let params = index.full_parameters_of(BUILTIN_ARRAY_URI, push_back.symbol.id);
    let types: Vec<_> = params.iter().map(|s| s.type_annotation.clone()).collect();
    assert_eq!(
        types,
        vec![Some(Type::from_annotation("T"))],
        "PushBack parameter must keep the placeholder type"
    );
}

#[test]
fn last_method_returns_placeholder() {
    let index = load_builtins_index();
    let last = index
        .direct_member_of("array", "Last", AccessLevel::Public)
        .expect("Last indexed");
    assert_eq!(
        last.symbol.type_annotation,
        Some(Type::from_annotation("T")),
        "Last must return the placeholder type"
    );
}

#[test]
fn map_class_is_indexed_with_placeholder_types() {
    let index = load_builtins_index();
    let def = index.find_top_level("map").expect("map class indexed");
    assert_eq!(
        def.symbol.kind,
        SymbolKind::Class,
        "map must be indexed as a class"
    );
    assert_eq!(
        def.symbol.origin,
        SymbolOrigin::Builtin,
        "map must carry the builtin origin"
    );
    assert_eq!(
        def.uri, BUILTIN_MAP_URI,
        "map must be indexed under the builtin map URI"
    );

    let key = index
        .direct_member_of("map", "Key", AccessLevel::Public)
        .expect("Key indexed");
    assert_eq!(
        key.symbol.type_annotation,
        Some(Type::from_annotation(MAP_KEY_PLACEHOLDER)),
        "Key must return the key placeholder type"
    );
}

#[test]
fn every_selection_range_slices_its_name_from_the_served_source() {
    let index = load_builtins_index();
    let mut checked = 0;
    for (uri, symbols) in index.documents() {
        let source = builtin_source(uri).unwrap_or_else(|| panic!("{uri} served"));
        for symbol in symbols {
            assert_eq!(
                &source[symbol.selection_byte_range.clone()],
                symbol.name,
                "selection range must slice the symbol name in {uri}"
            );
            checked += 1;
        }
    }
    assert!(
        checked > 30_000,
        "expected over 30,000 builtin symbols checked, got {checked}"
    );
}

#[test]
fn each_native_type_has_its_own_document() {
    assert!(builtin_source(&builtin_uri("CObject")).is_some());
    assert!(builtin_source(&builtin_uri("EScriptQueryFlags")).is_some());
    assert!(builtin_source(&builtin_uri("globals")).is_some());
}

#[test]
fn native_classes_carry_hierarchy_and_typed_fields() {
    let index = load_builtins_index();
    assert_eq!(
        index.superclass_of("C2dArray").as_deref(),
        Some("CResource")
    );
    let headers = index
        .direct_native_member_of("C2dArray", "headers")
        .expect("headers field indexed");
    assert_eq!(headers.symbol.kind, SymbolKind::Field);
    assert_eq!(headers.uri, builtin_uri("C2dArray"));
    assert_eq!(
        headers.symbol.type_annotation,
        Some(Type::from_annotation("array<String>"))
    );
}

#[rstest::rstest]
#[case::object_class("CObject", SymbolKind::Class)]
#[case::value_struct("Vector", SymbolKind::Struct)]
#[case::behaviour_tree_value("CBehTreeValFloat", SymbolKind::Struct)]
#[case::declared_enum("EAttackDirection", SymbolKind::Enum)]
#[case::undeclared_bitfield("ELightChannel", SymbolKind::Enum)]
fn native_types_are_usable_directly(#[case] name: &str, #[case] kind: SymbolKind) {
    let def = load_builtins_index()
        .find_top_level(name)
        .unwrap_or_else(|| panic!("{name} indexed"));
    assert_eq!(def.symbol.kind, kind);
    assert_eq!(def.symbol.origin, SymbolOrigin::Native);
    assert_eq!(def.uri, builtin_uri(name));
}

#[test]
fn native_functions_and_members_stay_out_of_ordinary_lookup() {
    let index = load_builtins_index();
    assert!(index.find_top_level("AbsF").is_none());
    assert!(index.find_native_top_level("AbsF").is_some());
    assert!(
        index
            .direct_member_of("C2dArray", "headers", AccessLevel::Private)
            .is_none()
    );
    assert!(
        index
            .direct_native_member_of("C2dArray", "headers")
            .is_some()
    );
}

#[rstest::rstest]
#[case::simple_type("CDateTime")]
#[case::string_like_simple_type("EngineTransform")]
fn simple_types_are_usable_directly(#[case] name: &str) {
    let def = load_builtins_index()
        .find_top_level(name)
        .unwrap_or_else(|| panic!("{name} indexed"));
    assert_eq!(def.symbol.kind, SymbolKind::NativeType);
    assert_eq!(def.uri, builtin_uri(name));
}

#[test]
fn native_enum_members_are_usable_directly() {
    let def = load_builtins_index()
        .find_enum_member("FLAG_ExcludePlayer")
        .expect("enum member resolves without an import");
    assert_eq!(def.uri, builtin_uri("EScriptQueryFlags"));
}
