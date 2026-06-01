use super::{Primitive, Type};

#[test]
fn parses_canonical_primitives() {
    assert_eq!(
        Type::from_annotation("int"),
        Type::Primitive(Primitive::Int)
    );
    assert_eq!(
        Type::from_annotation("string"),
        Type::Primitive(Primitive::String)
    );
    assert_eq!(Type::from_annotation("void"), Type::Void);
}

#[test]
fn canonicalises_engine_aliases() {
    assert_eq!(
        Type::from_annotation("Int32"),
        Type::Primitive(Primitive::Int)
    );
    assert_eq!(
        Type::from_annotation("CName"),
        Type::Primitive(Primitive::Name)
    );
}

#[test]
fn sized_engine_types_are_distinct_primitives() {
    assert_eq!(
        Type::from_annotation("Uint64"),
        Type::Primitive(Primitive::Uint64)
    );
    assert_eq!(
        Type::from_annotation("Int16"),
        Type::Primitive(Primitive::Int16)
    );
    assert_eq!(
        Type::from_annotation("StringAnsi"),
        Type::Primitive(Primitive::StringAnsi)
    );
}

#[test]
fn unrecognised_word_is_named() {
    assert_eq!(
        Type::from_annotation("CR4Player"),
        Type::Named("CR4Player".to_string())
    );
}

#[test]
fn empty_annotation_is_unknown() {
    assert_eq!(Type::from_annotation(""), Type::Unknown);
    assert_eq!(Type::from_annotation("   "), Type::Unknown);
}

#[test]
fn parses_array_element() {
    assert_eq!(
        Type::from_annotation("array<int>"),
        Type::Array(Box::new(Type::Primitive(Primitive::Int)))
    );
}

#[test]
fn parses_nested_array() {
    assert_eq!(
        Type::from_annotation("array<array<Foo>>"),
        Type::Array(Box::new(Type::Array(Box::new(Type::Named(
            "Foo".to_string()
        )))))
    );
}

#[test]
fn to_db_string_round_trips() {
    for s in [
        "int",
        "string",
        "void",
        "CR4Player",
        "array<int>",
        "array<array<Foo>>",
    ] {
        assert_eq!(Type::from_annotation(s).to_db_string().as_deref(), Some(s));
    }
}

#[test]
fn null_and_unknown_have_no_db_string() {
    assert_eq!(Type::Null.to_db_string(), None);
    assert_eq!(Type::Unknown.to_db_string(), None);
}

#[test]
fn display_matches_db_form() {
    assert_eq!(
        Type::from_annotation("array<Foo>").to_string(),
        "array<Foo>"
    );
    assert_eq!(Type::Null.to_string(), "NULL");
    assert_eq!(Type::Unknown.to_string(), "?");
}
