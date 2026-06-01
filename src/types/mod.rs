//! Structured value model for WitcherScript types.
//!
//! The rest of the crate carries types as bare `String` annotations
//! (`Symbol.type_annotation`). This module is the typed front-end: parse an
//! annotation into a [`Type`], reason about it, and collapse it back to the
//! string key the resolver's `SymbolDb` expects.

mod parse;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Void,
    Primitive(Primitive),
    /// A class, struct, enum, or state, by name.
    Named(String),
    Array(Box<Type>),
    /// The type of the `NULL` literal.
    Null,
    /// Not confidently known. Never produces a diagnostic.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    Bool,
    Byte,
    Int,
    Float,
    Name,
    String,
}

impl Type {
    /// Parse a `Symbol.type_annotation` / `SymbolDb` container string.
    ///
    /// Handles `array<...>` recursively and canonicalises engine aliases
    /// (`Int32`, `CName`, ...). An empty string is [`Type::Unknown`]; an
    /// unrecognised bare word is [`Type::Named`].
    pub fn from_annotation(s: &str) -> Type {
        parse::from_annotation(s)
    }

    /// Collapse back to the string `SymbolDb` uses as a lookup key
    /// (`find_member`, `find_top_level`). [`Type::Null`] and [`Type::Unknown`]
    /// have no container to look up and return `None`.
    pub fn to_db_string(&self) -> Option<String> {
        match self {
            Type::Void => Some("void".to_string()),
            Type::Primitive(p) => Some(p.canonical().to_string()),
            Type::Named(name) => Some(name.clone()),
            Type::Array(elem) => Some(format!("array<{}>", elem.to_db_string()?)),
            Type::Null | Type::Unknown => None,
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Type::Unknown)
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Void => f.write_str("void"),
            Type::Primitive(p) => f.write_str(p.canonical()),
            Type::Named(name) => f.write_str(name),
            Type::Array(elem) => write!(f, "array<{elem}>"),
            Type::Null => f.write_str("NULL"),
            Type::Unknown => f.write_str("?"),
        }
    }
}

impl Primitive {
    pub fn canonical(self) -> &'static str {
        match self {
            Primitive::Bool => "bool",
            Primitive::Byte => "byte",
            Primitive::Int => "int",
            Primitive::Float => "float",
            Primitive::Name => "name",
            Primitive::String => "string",
        }
    }
}
