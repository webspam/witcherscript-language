use crate::test_support::TestDb;
use crate::types::{Primitive, Type};

use super::{assignability, Assignability, CastKind};

const TYPES_SRC: &str =
    "class Base {} class Derived extends Base {} class Other {} enum Mood { Happy, Sad }\n";

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
fn numeric_widening_is_implicit() {
    let t = TestDb::new(TYPES_SRC);
    for (from, to) in [
        (Primitive::Byte, Primitive::Int),
        (Primitive::Byte, Primitive::Float),
        (Primitive::Int, Primitive::Float),
    ] {
        assert_eq!(
            assignability(&prim(from), &prim(to), &t.db()),
            Assignability::ImplicitCast(CastKind::NumericWidening),
            "{from:?} -> {to:?}"
        );
    }
}

#[test]
fn to_string_is_implicit() {
    let t = TestDb::new(TYPES_SRC);
    for from in [
        Primitive::Float,
        Primitive::Int,
        Primitive::Byte,
        Primitive::Bool,
        Primitive::Name,
    ] {
        assert_eq!(
            assignability(&prim(from), &prim(Primitive::String), &t.db()),
            Assignability::ImplicitCast(CastKind::ToString),
            "{from:?} -> string"
        );
    }
}

#[test]
fn narrowing_and_unrelated_primitives_are_incompatible() {
    let t = TestDb::new(TYPES_SRC);
    for (from, to) in [
        (Primitive::Float, Primitive::Int),
        (Primitive::String, Primitive::Int),
        (Primitive::Int, Primitive::Bool),
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
