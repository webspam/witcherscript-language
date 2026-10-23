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

const DEFAULT_COERCION_ACCEPTS: &[(&str, &[Primitive])] = &[
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
    if let Some((ctor, args)) = parse_generic_type(trimmed) {
        match (ctor, split_type_args(args).as_slice()) {
            ("array", [element]) => return Type::Array(Box::new(from_annotation(element))),
            ("map", [key, value]) => {
                return Type::Map(
                    Box::new(from_annotation(key)),
                    Box::new(from_annotation(value)),
                );
            }
            _ => {}
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

fn split_type_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in args.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(args[start..].trim());
    parts
}

pub fn is_builtin_type_name(name: &str) -> bool {
    name == "void" || PRIMITIVE_ALIASES.iter().any(|(alias, _)| *alias == name)
}

pub(crate) fn default_coercion_accepts(name: &str) -> Option<&'static [Primitive]> {
    DEFAULT_COERCION_ACCEPTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, prims)| *prims)
}
