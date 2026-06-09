use super::*;
use crate::symbols::SymbolKind;
use crate::types::Type;

#[test]
fn array_class_is_indexed() {
    let index = load_builtins_index();
    let def = index.find_top_level("array").expect("array class indexed");
    assert_eq!(def.symbol.kind, SymbolKind::Class);
    assert_eq!(def.uri, BUILTIN_ARRAY_URI);
}

#[test]
fn array_members_are_indexed_with_placeholder_types() {
    let index = load_builtins_index();
    let push_back = index
        .direct_member_of("array", "PushBack", crate::symbols::AccessLevel::Public)
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
        .direct_member_of("array", "Last", crate::symbols::AccessLevel::Public)
        .expect("Last indexed");
    assert_eq!(
        last.symbol.type_annotation,
        Some(Type::from_annotation("T")),
        "Last must return the placeholder type"
    );
}
