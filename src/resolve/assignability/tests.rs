use crate::test_support::TestDb;
use crate::types::{Primitive, Type};

use super::{assignability, Assignability, CastKind};

const TYPES_SRC: &str = "class Base {} class Derived extends Base {} class Other {} \
     enum Mood { Happy, Sad } struct Vec { var x : float; } \
     statemachine class Machine {} state Active in Machine {}\n";

fn prim(p: Primitive) -> Type {
    Type::Primitive(p)
}

#[test]
fn identical_primitives_are_identical() {
    let t = TestDb::new(TYPES_SRC);
    assert_eq!(
        assignability(&prim(Primitive::Int), &prim(Primitive::Int), &t.db()),
        Assignability::Identical
    );
}

#[test]
fn implicit_primitive_casts_match_compiler() {
    let t = TestDb::new(TYPES_SRC);
    for (from, to) in [
        (Primitive::Byte, Primitive::Int),
        (Primitive::Byte, Primitive::Float),
        (Primitive::Int, Primitive::Float),
        (Primitive::Int, Primitive::Byte),
        (Primitive::Int, Primitive::Bool),
        (Primitive::Float, Primitive::Bool),
        (Primitive::String, Primitive::Bool),
        (Primitive::Bool, Primitive::String),
        (Primitive::Name, Primitive::String),
    ] {
        assert_eq!(
            assignability(&prim(from), &prim(to), &t.db()),
            Assignability::ImplicitCast(CastKind::Primitive),
            "{from:?} -> {to:?}"
        );
    }
}

#[test]
fn explicit_only_and_unrelated_primitives_are_incompatible() {
    let t = TestDb::new(TYPES_SRC);
    for (from, to) in [
        (Primitive::Float, Primitive::Int),
        (Primitive::String, Primitive::Int),
        (Primitive::Bool, Primitive::Byte),
        (Primitive::String, Primitive::Name),
    ] {
        assert_eq!(
            assignability(&prim(from), &prim(to), &t.db()),
            Assignability::Incompatible,
            "{from:?} -> {to:?}"
        );
    }
}

#[test]
fn sized_ints_accept_only_their_own_spelling() {
    let t = TestDb::new(TYPES_SRC);
    assert_eq!(
        assignability(&prim(Primitive::Int16), &prim(Primitive::Int16), &t.db()),
        Assignability::Identical
    );
    for other in [Primitive::Int, Primitive::Int8, Primitive::Uint16] {
        assert_eq!(
            assignability(&prim(other), &prim(Primitive::Int16), &t.db()),
            Assignability::Incompatible,
            "{other:?} -> Int16"
        );
        assert_eq!(
            assignability(&prim(Primitive::Int16), &prim(other), &t.db()),
            Assignability::Incompatible,
            "Int16 -> {other:?}"
        );
    }
}

#[test]
fn derived_upcasts_to_base() {
    let t = TestDb::new(TYPES_SRC);
    assert_eq!(
        assignability(
            &Type::Named("Derived".into()),
            &Type::Named("Base".into()),
            &t.db()
        ),
        Assignability::Identical
    );
}

#[test]
fn base_does_not_downcast_to_derived() {
    let t = TestDb::new(TYPES_SRC);
    assert_eq!(
        assignability(
            &Type::Named("Base".into()),
            &Type::Named("Derived".into()),
            &t.db()
        ),
        Assignability::Incompatible
    );
}

#[test]
fn unrelated_named_are_incompatible() {
    let t = TestDb::new(TYPES_SRC);
    assert_eq!(
        assignability(
            &Type::Named("Other".into()),
            &Type::Named("Base".into()),
            &t.db()
        ),
        Assignability::Incompatible
    );
}

#[test]
fn null_assigns_to_named_but_not_primitive() {
    let t = TestDb::new(TYPES_SRC);
    assert_eq!(
        assignability(&Type::Null, &Type::Named("Base".into()), &t.db()),
        Assignability::ImplicitCast(CastKind::NullToRef)
    );
    assert_eq!(
        assignability(&Type::Null, &prim(Primitive::Int), &t.db()),
        Assignability::Incompatible
    );
}

#[test]
fn arrays_are_invariant() {
    let t = TestDb::new(TYPES_SRC);
    let array_int = Type::Array(Box::new(prim(Primitive::Int)));
    let array_float = Type::Array(Box::new(prim(Primitive::Float)));
    assert_eq!(
        assignability(&array_int, &array_int.clone(), &t.db()),
        Assignability::Identical
    );
    assert_eq!(
        assignability(&array_int, &array_float, &t.db()),
        Assignability::Incompatible
    );
}

#[test]
fn objects_cast_to_bool_and_string() {
    let t = TestDb::new(TYPES_SRC);
    for name in ["Base", "Derived", "Active"] {
        assert_eq!(
            assignability(&Type::Named(name.into()), &prim(Primitive::Bool), &t.db()),
            Assignability::ImplicitCast(CastKind::ObjectToBool),
            "{name} -> bool"
        );
        assert_eq!(
            assignability(&Type::Named(name.into()), &prim(Primitive::String), &t.db()),
            Assignability::ImplicitCast(CastKind::ToString),
            "{name} -> string"
        );
    }
}

#[test]
fn enums_stringify_but_do_not_cast_to_bool() {
    let t = TestDb::new(TYPES_SRC);
    assert_eq!(
        assignability(
            &Type::Named("Mood".into()),
            &prim(Primitive::String),
            &t.db()
        ),
        Assignability::ImplicitCast(CastKind::ToString)
    );
    assert_eq!(
        assignability(&Type::Named("Mood".into()), &prim(Primitive::Bool), &t.db()),
        Assignability::Incompatible
    );
}

#[test]
fn structs_do_not_cast_to_bool_or_string() {
    let t = TestDb::new(TYPES_SRC);
    for to in [Primitive::Bool, Primitive::String] {
        assert_eq!(
            assignability(&Type::Named("Vec".into()), &prim(to), &t.db()),
            Assignability::Incompatible,
            "Vec -> {to:?}"
        );
    }
}

#[test]
fn bool_and_string_do_not_cast_to_object() {
    let t = TestDb::new(TYPES_SRC);
    for from in [Primitive::Bool, Primitive::String] {
        assert_eq!(
            assignability(&prim(from), &Type::Named("Base".into()), &t.db()),
            Assignability::Incompatible,
            "{from:?} -> Base"
        );
    }
}

#[test]
fn enum_and_int_convert_both_ways() {
    let t = TestDb::new(TYPES_SRC);
    assert_eq!(
        assignability(&Type::Named("Mood".into()), &prim(Primitive::Int), &t.db()),
        Assignability::ImplicitCast(CastKind::EnumInt)
    );
    assert_eq!(
        assignability(&prim(Primitive::Int), &Type::Named("Mood".into()), &t.db()),
        Assignability::ImplicitCast(CastKind::EnumInt)
    );
}
