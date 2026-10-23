//! Structured value model for `WitcherScript` types: the typed front-end over bare-`String` `type_annotation`s.

mod parse;

#[cfg(test)]
mod tests;

pub(crate) use parse::default_coercion_accepts;
pub use parse::is_builtin_type_name;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Void,
    Primitive(Primitive),
    /// A class, struct, enum, or state, by name.
    Named(String),
    Array(Box<Type>),
    Map(Box<Type>, Box<Type>),
    /// The type of the `NULL` literal.
    Null,
    /// Not confidently known. Never produces a diagnostic.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    Bool,
    Byte,
    Int,
    Int16,
    Int8,
    Uint16,
    Uint32,
    Uint64,
    Float,
    Name,
    String,
    StringAnsi,
}

impl Type {
    /// Parse a type annotation, e.g. `int`, `array<CEntity>`, or `map<name, int>`.
    pub fn from_annotation(s: &str) -> Type {
        parse::from_annotation(s)
    }

    /// Collapse to the `SymbolDb` lookup key. `Null`/`Unknown` have no container -> `None`.
    pub(crate) fn to_db_string(&self) -> Option<String> {
        match self {
            Type::Void => Some("void".to_string()),
            Type::Primitive(p) => Some(p.canonical().to_string()),
            Type::Named(name) => Some(name.clone()),
            Type::Array(elem) => Some(format!("array<{}>", elem.to_db_string()?)),
            Type::Map(key, value) => Some(format!(
                "map<{}, {}>",
                key.to_db_string()?,
                value.to_db_string()?
            )),
            Type::Null | Type::Unknown => None,
        }
    }

    /// The top-level `SymbolDb` lookup name: generic arguments are dropped, e.g. `array<int>` -> `array`.
    pub(crate) fn to_lookup_ctor(&self) -> Option<String> {
        let raw = self.to_db_string()?;
        Some(match parse_generic_type(&raw) {
            Some((ctor, _)) => ctor.to_string(),
            None => raw,
        })
    }

    pub(crate) fn substitute_placeholders(&self, bindings: &[(&str, &Type)]) -> Type {
        match self {
            Type::Named(name) => bindings
                .iter()
                .find(|(placeholder, _)| placeholder == name)
                .map_or_else(|| self.clone(), |(_, replacement)| (*replacement).clone()),
            Type::Array(elem) => Type::Array(Box::new(elem.substitute_placeholders(bindings))),
            Type::Map(key, value) => Type::Map(
                Box::new(key.substitute_placeholders(bindings)),
                Box::new(value.substitute_placeholders(bindings)),
            ),
            Type::Void | Type::Primitive(_) | Type::Null | Type::Unknown => self.clone(),
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Void => f.write_str("void"),
            Type::Primitive(p) => f.write_str(p.canonical()),
            Type::Named(name) => f.write_str(name),
            Type::Array(elem) => write!(f, "array<{elem}>"),
            Type::Map(key, value) => write!(f, "map<{key}, {value}>"),
            Type::Null => f.write_str("NULL"),
            Type::Unknown => f.write_str("?"),
        }
    }
}

pub(crate) fn parse_generic_type(s: &str) -> Option<(&str, &str)> {
    let trimmed = s.trim();
    let lt = trimmed.find('<')?;
    if !trimmed.ends_with('>') {
        return None;
    }
    let ctor = trimmed[..lt].trim();
    let element = trimmed[lt + 1..trimmed.len() - 1].trim();
    if ctor.is_empty() || element.is_empty() {
        return None;
    }
    Some((ctor, element))
}

pub(crate) fn split_type_args(args: &str) -> Vec<&str> {
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

impl Primitive {
    pub(crate) fn canonical(self) -> &'static str {
        match self {
            Primitive::Bool => "bool",
            Primitive::Byte => "byte",
            Primitive::Int => "int",
            Primitive::Int16 => "Int16",
            Primitive::Int8 => "Int8",
            Primitive::Uint16 => "Uint16",
            Primitive::Uint32 => "Uint32",
            Primitive::Uint64 => "Uint64",
            Primitive::Float => "float",
            Primitive::Name => "name",
            Primitive::String => "string",
            Primitive::StringAnsi => "StringAnsi",
        }
    }
}
