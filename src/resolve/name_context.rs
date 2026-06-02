use tree_sitter::Node;

use crate::cst::nav::nth_child_kind;
use crate::symbols::SymbolKind;

/// The syntactic position of an identifier, used to restrict which symbol kinds
/// a name lookup is allowed to return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameContext {
    /// `: T`, `new T(...)`, cast `(T)`, `class extends T`, `state X in T`,
    /// `@addMethod(T)`. Accepts class, struct, enum.
    Type,
    /// `state X in Owner extends T`. Accepts only a state declared in `owner_class`
    /// or one of its statemachine ancestors.
    StateExtends { owner_class: String },
    /// Bare call `Name(...)`. Accepts function, event, struct (constructor).
    Callable,
    /// Bare identifier in expression position. Accepts function, event, class,
    /// struct, enum. States are never bare values.
    Value,
}

impl NameContext {
    /// True when a top-level symbol of the given kind is a legal candidate
    /// for this context. `StateExtends` requires a separate owner-chain lookup
    /// (so this returns true only for `State`).
    pub fn accepts(&self, kind: SymbolKind) -> bool {
        match self {
            NameContext::Type => matches!(
                kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum
            ),
            NameContext::StateExtends { .. } => kind == SymbolKind::State,
            NameContext::Callable => matches!(
                kind,
                SymbolKind::Function | SymbolKind::Event | SymbolKind::Struct
            ),
            NameContext::Value => kind != SymbolKind::State,
        }
    }
}

/// Classify an identifier's CST position into a `NameContext`. Returns `None`
/// for positions that are not kind-aware top-level lookups (declarations,
/// member accesses, default/hint members) - those resolve via separate paths.
pub fn classify_ident_context(ident: Node, source: &[u8]) -> Option<NameContext> {
    let parent = ident.parent()?;

    if is_declaration(ident, parent) {
        return None;
    }

    if let Some(ctx) = type_reference_context(ident, parent, source) {
        return Some(ctx);
    }

    if crate::cst::grammar::ident_default_or_hint_kind(ident).is_some() {
        return None;
    }

    if parent.kind() == "member_access_expr" {
        let is_member = parent.child_by_field_name("member").map(|n| n.id()) == Some(ident.id());
        if is_member {
            return None;
        }
    }

    if parent.kind() == "func_call_expr"
        && parent.child_by_field_name("func").map(|n| n.id()) == Some(ident.id())
    {
        return Some(NameContext::Callable);
    }

    Some(NameContext::Value)
}

fn is_declaration(ident: Node, parent: Node) -> bool {
    match parent.kind() {
        "class_decl" | "struct_decl" | "enum_decl" | "state_decl" | "func_decl" | "event_decl"
        | "autobind_decl" | "enum_decl_variant" => {
            parent.child_by_field_name("name").map(|n| n.id()) == Some(ident.id())
        }
        "func_param_group" | "local_var_decl_stmt" | "member_var_decl" => {
            let mut cursor = parent.walk();
            let found = parent
                .children_by_field_name("names", &mut cursor)
                .any(|n| n.id() == ident.id());
            found
        }
        _ => false,
    }
}

fn type_reference_context(ident: Node, parent: Node, source: &[u8]) -> Option<NameContext> {
    match parent.kind() {
        "state_decl" => {
            if parent.child_by_field_name("base").map(|n| n.id()) == Some(ident.id()) {
                let owner_ident = nth_child_kind(parent, "ident", 1)?;
                let owner_class = owner_ident.utf8_text(source).ok()?.to_string();
                return Some(NameContext::StateExtends { owner_class });
            }
            if parent.child_by_field_name("parent").map(|n| n.id()) == Some(ident.id()) {
                return Some(NameContext::Type);
            }
            None
        }
        "class_decl" => {
            let is_base = parent.child_by_field_name("base").map(|n| n.id()) == Some(ident.id());
            let is_parent =
                parent.child_by_field_name("parent").map(|n| n.id()) == Some(ident.id());
            (is_base || is_parent).then_some(NameContext::Type)
        }
        "type_annot" => (parent.child_by_field_name("type_name").map(|n| n.id())
            == Some(ident.id()))
        .then_some(NameContext::Type),
        "new_expr" => (parent.child_by_field_name("class").map(|n| n.id()) == Some(ident.id()))
            .then_some(NameContext::Type),
        "annotation" => (parent.child_by_field_name("arg").map(|n| n.id()) == Some(ident.id()))
            .then_some(NameContext::Type),
        "cast_expr" => {
            let mut cursor = parent.walk();
            let found = parent
                .children_by_field_name("type", &mut cursor)
                .any(|n| n.id() == ident.id());
            found.then_some(NameContext::Type)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
