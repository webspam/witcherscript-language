use tree_sitter::Node;

use crate::cst::nav::nth_child_kind;
use crate::cst::{fields, kinds};
use crate::symbols::SymbolKind;

/// The syntactic position of an identifier, used to restrict which symbol kinds
/// a name lookup is allowed to return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameContext {
    /// `: T`, `new T(...)`, cast `(T)`, `class extends T`, `state X in T`,
    /// `@addMethod(T)`. Accepts class, native type, struct, enum.
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
            NameContext::Type => kind.is_assignable_type(),
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

    if parent.kind() == kinds::MEMBER_ACCESS_EXPR {
        let is_member =
            parent.child_by_field_name(fields::MEMBER).map(|n| n.id()) == Some(ident.id());
        if is_member {
            return None;
        }
    }

    if parent.kind() == kinds::FUNC_CALL_EXPR
        && parent.child_by_field_name(fields::FUNC).map(|n| n.id()) == Some(ident.id())
    {
        return Some(NameContext::Callable);
    }

    Some(NameContext::Value)
}

fn is_declaration(ident: Node, parent: Node) -> bool {
    match parent.kind() {
        kinds::CLASS_DECL
        | kinds::STRUCT_DECL
        | kinds::ENUM_DECL
        | kinds::STATE_DECL
        | kinds::FUNC_DECL
        | kinds::EVENT_DECL
        | kinds::AUTOBIND_DECL
        | kinds::ENUM_DECL_VARIANT => {
            parent.child_by_field_name(fields::NAME).map(|n| n.id()) == Some(ident.id())
        }
        kinds::FUNC_PARAM_GROUP | kinds::LOCAL_VAR_DECL_STMT | kinds::MEMBER_VAR_DECL => {
            let mut cursor = parent.walk();

            parent
                .children_by_field_name(fields::NAMES, &mut cursor)
                .any(|n| n.id() == ident.id())
        }
        _ => false,
    }
}

fn type_reference_context(ident: Node, parent: Node, source: &[u8]) -> Option<NameContext> {
    match parent.kind() {
        kinds::STATE_DECL => {
            if parent.child_by_field_name(fields::BASE).map(|n| n.id()) == Some(ident.id()) {
                let owner_ident = nth_child_kind(parent, kinds::IDENT, 1)?;
                let owner_class = owner_ident.utf8_text(source).ok()?.to_string();
                return Some(NameContext::StateExtends { owner_class });
            }
            if parent.child_by_field_name(fields::PARENT).map(|n| n.id()) == Some(ident.id()) {
                return Some(NameContext::Type);
            }
            None
        }
        kinds::CLASS_DECL => {
            let is_base =
                parent.child_by_field_name(fields::BASE).map(|n| n.id()) == Some(ident.id());
            let is_parent =
                parent.child_by_field_name(fields::PARENT).map(|n| n.id()) == Some(ident.id());
            (is_base || is_parent).then_some(NameContext::Type)
        }
        kinds::TYPE_ANNOT => (parent
            .child_by_field_name(fields::TYPE_NAME)
            .map(|n| n.id())
            == Some(ident.id()))
        .then_some(NameContext::Type),
        kinds::NEW_EXPR => (parent.child_by_field_name(fields::CLASS).map(|n| n.id())
            == Some(ident.id()))
        .then_some(NameContext::Type),
        kinds::ANNOTATION => (parent.child_by_field_name(fields::ARG).map(|n| n.id())
            == Some(ident.id()))
        .then_some(NameContext::Type),
        kinds::CAST_EXPR => (parent.child_by_field_name(fields::TYPE).map(|n| n.id())
            == Some(ident.id()))
        .then_some(NameContext::Type),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
