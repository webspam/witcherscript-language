use super::parse_generic_type;

use super::{Primitive, Type};

/// Aliases mapping onto a [`Primitive`]; mirrors `resolve::ast::BUILTIN_TYPES`.
const PRIMITIVE_ALIASES: &[(&str, Primitive)] = &[
    ("bool", Primitive::Bool),
    ("Bool", Primitive::Bool),
    ("byte", Primitive::Byte),
    ("Uint8", Primitive::Byte),
    ("int", Primitive::Int),
    ("Int32", Primitive::Int),
    ("Int16", Primitive::Int),
    ("Int8", Primitive::Int),
    ("Uint16", Primitive::Int),
    ("Uint32", Primitive::Int),
    ("Uint64", Primitive::Int),
    ("float", Primitive::Float),
    ("Float", Primitive::Float),
    ("name", Primitive::Name),
    ("CName", Primitive::Name),
    ("string", Primitive::String),
    ("String", Primitive::String),
    ("StringAnsi", Primitive::String),
];

pub(super) fn from_annotation(s: &str) -> Type {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Type::Unknown;
    }
    if trimmed == "void" {
        return Type::Void;
    }
    if let Some((ctor, element)) = parse_generic_type(trimmed) {
        if ctor == "array" {
            return Type::Array(Box::new(from_annotation(element)));
        }
    }
    if let Some(prim) = PRIMITIVE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == trimmed)
        .map(|(_, p)| *p)
    {
        return Type::Primitive(prim);
    }
    Type::Named(trimmed.to_string())
}
