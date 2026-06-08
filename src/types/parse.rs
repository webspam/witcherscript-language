use super::parse_generic_type;

use super::{Primitive, Type};

/// Source of truth for primitive spellings; `is_builtin_type_name` derives the builtin set from it.
const PRIMITIVE_ALIASES: &[(&str, Primitive)] = &[
    ("bool", Primitive::Bool),
    ("Bool", Primitive::Bool),
    ("byte", Primitive::Byte),
    ("Uint8", Primitive::Byte),
    ("int", Primitive::Int),
    ("Int32", Primitive::Int),
    ("Int16", Primitive::Int16),
    ("Int8", Primitive::Int8),
    ("Uint16", Primitive::Uint16),
    ("Uint32", Primitive::Uint32),
    ("Uint64", Primitive::Uint64),
    ("float", Primitive::Float),
    ("Float", Primitive::Float),
    ("name", Primitive::Name),
    ("CName", Primitive::Name),
    ("string", Primitive::String),
    ("String", Primitive::String),
    ("StringAnsi", Primitive::StringAnsi),
];

/// Per-type `default`-value acceptance for the `CBehTreeVal*` native types (Float also takes `int`).
const NATIVE_TYPE_ACCEPTS: &[(&str, &[Primitive])] = &[
    ("CBehTreeValBool", &[Primitive::Bool]),
    ("CBehTreeValInt", &[Primitive::Int]),
    ("CBehTreeValFloat", &[Primitive::Int, Primitive::Float]),
    ("CBehTreeValString", &[Primitive::String]),
    ("CBehTreeValCName", &[Primitive::Name]),
];

pub(super) fn from_annotation(s: &str) -> Type {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Type::Unknown;
    }
    if trimmed == "void" {
        return Type::Void;
    }
    if let Some((ctor, element)) = parse_generic_type(trimmed)
        && ctor == "array"
    {
        return Type::Array(Box::new(from_annotation(element)));
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

pub(crate) fn is_builtin_type_name(name: &str) -> bool {
    name == "void" || PRIMITIVE_ALIASES.iter().any(|(alias, _)| *alias == name)
}

pub(crate) fn native_type_accepts(name: &str) -> Option<&'static [Primitive]> {
    NATIVE_TYPE_ACCEPTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, prims)| *prims)
}

pub(crate) fn native_type_names() -> impl Iterator<Item = &'static str> {
    NATIVE_TYPE_ACCEPTS.iter().map(|(name, _)| *name)
}
