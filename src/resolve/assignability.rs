//! Pure type-compatibility engine.
//!
//! Decides whether a value of one [`Type`] may flow into a slot of another,
//! and if so whether an implicit conversion is involved. The allowed implicit
//! conversions live in a single table ([`IMPLICIT_PRIMITIVE_CASTS`]) so the
//! permitted set is tunable in one place.

use crate::symbols::SymbolKind;
use crate::types::{Primitive, Type};

use super::SymbolDb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastKind {
    /// Lossless numeric promotion (`byte`->`int`, `int`->`float`, ...).
    NumericWidening,
    /// Scalar stringified into a `string` slot.
    ToString,
    /// `enum` <-> `int` in either direction (enums are int-backed).
    EnumInt,
    /// `NULL` into a reference-typed (`Named`) slot.
    NullToRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assignability {
    Identical,
    ImplicitCast(CastKind),
    Incompatible,
}

/// Implicit primitive conversions the language performs silently.
///
/// Single source of truth for the allowed-cast set. Seeded with the
/// well-known WitcherScript conversions; the exact set still wants
/// confirmation against the compiler.
const IMPLICIT_PRIMITIVE_CASTS: &[(Primitive, Primitive)] = &[
    (Primitive::Byte, Primitive::Int),
    (Primitive::Byte, Primitive::Float),
    (Primitive::Int, Primitive::Float),
    (Primitive::Int, Primitive::String),
    (Primitive::Float, Primitive::String),
    (Primitive::Byte, Primitive::String),
    (Primitive::Bool, Primitive::String),
    (Primitive::Name, Primitive::String),
];

/// Whether a value of type `from` may be assigned into a slot of type `to`.
///
/// Contract: callers guarantee neither side is [`Type::Unknown`]; an `Unknown`
/// type carries no information and must be filtered out before reporting.
pub fn assignability(from: &Type, to: &Type, db: &SymbolDb) -> Assignability {
    if from == to {
        return Assignability::Identical;
    }

    match (from, to) {
        (Type::Unknown, _) | (_, Type::Unknown) => Assignability::Incompatible,
        (Type::Void, _) | (_, Type::Void) => Assignability::Incompatible,

        (Type::Null, Type::Named(_)) => Assignability::ImplicitCast(CastKind::NullToRef),
        (Type::Null, _) => Assignability::Incompatible,

        (Type::Named(f), Type::Named(t)) => named_assignability(f, t, db),

        (Type::Array(fe), Type::Array(te)) => match assignability(fe, te, db) {
            Assignability::Identical => Assignability::Identical,
            _ => Assignability::Incompatible,
        },
        (Type::Array(_), _) | (_, Type::Array(_)) => Assignability::Incompatible,

        (Type::Named(n), Type::Primitive(Primitive::Int))
        | (Type::Primitive(Primitive::Int), Type::Named(n))
            if is_enum(db, n) =>
        {
            Assignability::ImplicitCast(CastKind::EnumInt)
        }

        (Type::Primitive(pf), Type::Primitive(pt)) => primitive_assignability(*pf, *pt),

        _ => Assignability::Incompatible,
    }
}

fn named_assignability(from: &str, to: &str, db: &SymbolDb) -> Assignability {
    if db.inherits_from(from, to) {
        Assignability::Identical
    } else {
        Assignability::Incompatible
    }
}

fn primitive_assignability(from: Primitive, to: Primitive) -> Assignability {
    if IMPLICIT_PRIMITIVE_CASTS.contains(&(from, to)) {
        let kind = if to == Primitive::String {
            CastKind::ToString
        } else {
            CastKind::NumericWidening
        };
        Assignability::ImplicitCast(kind)
    } else {
        Assignability::Incompatible
    }
}

fn is_enum(db: &SymbolDb, name: &str) -> bool {
    db.find_top_level(name)
        .is_some_and(|d| d.symbol.kind == SymbolKind::Enum)
}

#[cfg(test)]
mod tests;
