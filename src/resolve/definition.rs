use tree_sitter::Node;

use crate::cst::grammar::{call_callee, callee_ident};
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{Symbol, SymbolKind};

use super::ast::{
    find_ancestor_of_kind, first_named_child, identifier_at, nodes_at_offset,
    significant_node_before_byte,
};
use super::inference::{
    enclosing_type_context, resolve_document_top_level, resolve_member_access, resolve_name,
    resolve_name_in_context,
};
use super::name_context::classify_ident_context;
use super::symbol_db::SymbolDb;
use super::{Definition, annotation_target_class, dedup_definitions};

pub fn resolve_definition(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Definition> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    resolve_definition_at_byte(uri, document, db, byte_offset)
}

pub fn resolve_definition_at_byte(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<Definition> {
    if let Some(def) = resolve_self_keyword(uri, document, db, byte_offset) {
        return Some(def);
    }

    let ident = identifier_at(document.tree.root_node(), byte_offset)?;
    resolve_for_ident(uri, document, db, ident, byte_offset)
}

pub(crate) fn callee_params(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    call: Node,
) -> Option<Vec<Symbol>> {
    let ident = callee_ident(call_callee(call)?)?;
    let def = resolve_definition_at_byte(uri, document, db, ident.start_byte())?;
    if !def.symbol.kind.is_callable() {
        return None;
    }
    Some(db.full_parameters_of(&def.uri, def.symbol.id))
}

pub fn resolve_definition_at_ident(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    ident: Node,
) -> Option<Definition> {
    resolve_for_ident(uri, document, db, ident, ident.start_byte())
}

pub fn classify_definition_at_ident(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    ident: Node,
) -> Option<Definition> {
    resolve_for_ident_no_site_fallback(uri, document, db, ident, ident.start_byte())
}

fn resolve_for_ident(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    ident: Node,
    byte_offset: usize,
) -> Option<Definition> {
    let name = ident.utf8_text(document.source.as_bytes()).ok()?;
    if let Some(def) = resolve_wrapped_method(document, db, ident, byte_offset, name) {
        return Some(def);
    }
    resolve_at_definition_site(uri, document, byte_offset, name)
        .or_else(|| resolve_for_ident_no_site_fallback(uri, document, db, ident, byte_offset))
}

const WRAPPED_METHOD_MACRO: &str = "wrappedMethod";

/// `wrappedMethod()` inside a `@wrapMethod(Class)` body navigates to the method it wraps.
fn resolve_wrapped_method(
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    ident: Node,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    if !is_wrapped_method_macro(ident, name) {
        return None;
    }
    let callable = document
        .symbols
        .enclosing_symbol_at(byte_offset, &[SymbolKind::Function])?;
    let target = wrap_method_target_class(callable)?;
    db.find_class_body_member(target, &callable.name)
}

fn is_wrapped_method_macro(ident: Node, name: &str) -> bool {
    if name != WRAPPED_METHOD_MACRO {
        return false;
    }
    // `this.wrappedMethod()` is an ordinary member call, not a macro.
    ident
        .parent()
        .is_none_or(|p| p.kind() != kinds::MEMBER_ACCESS_EXPR)
}

fn wrap_method_target_class(symbol: &Symbol) -> Option<&str> {
    symbol
        .annotations
        .iter()
        .find(|a| a.name == "wrapMethod")
        .and_then(|a| a.argument.as_deref())
}

fn resolve_for_ident_no_site_fallback(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    ident: Node,
    byte_offset: usize,
) -> Option<Definition> {
    let name = ident.utf8_text(document.source.as_bytes()).ok()?;

    if let Some(member_access) = ident
        .parent()
        .filter(|p| p.kind() == kinds::MEMBER_ACCESS_EXPR)
    {
        let is_receiver = first_named_child(member_access).is_some_and(|r| r.id() == ident.id());
        if !is_receiver {
            return resolve_member_access(uri, document, db, ident, name);
        }
    }

    if let Some(ctx) = classify_ident_context(ident, document.source.as_bytes()) {
        return resolve_name_in_context(uri, document, db, byte_offset, name, &ctx);
    }

    resolve_name(uri, document, db, byte_offset, name)
}

/// All declaration sites at `position`, class-body declaration first.
pub fn resolve_all_definitions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    let Some(byte_offset) = document
        .line_index
        .position_to_byte(&document.source, position)
    else {
        return Vec::new();
    };
    // this/super/parent never wrap, so the macro suppression below does not apply.
    if let Some(primary) = resolve_self_keyword(uri, document, db, byte_offset) {
        return all_declarations_of(&primary, db);
    }
    let Some(ident) = resolution_ident(document, byte_offset) else {
        return Vec::new();
    };
    let Some(primary) = resolve_definition_at_ident(uri, document, db, ident) else {
        return Vec::new();
    };
    // Suppression keys off the resolved ident, not the cursor offset, so a trailing-`;` park stays consistent.
    if ident
        .utf8_text(document.source.as_bytes())
        .is_ok_and(|name| is_wrapped_method_macro(ident, name))
    {
        return vec![primary];
    }
    all_declarations_of(&primary, db)
}

fn resolution_ident(document: &ParsedDocument, byte_offset: usize) -> Option<Node<'_>> {
    let root = document.tree.root_node();
    identifier_at(root, byte_offset)
        .or_else(|| ident_before_trailing_semicolon(document, byte_offset))
}

/// Cursor parked past a line-ending `;` resolves nothing, yet the identifier left of it is the obvious intent.
/// Must only be used with user-initiated requests, e.g. Go To Definition.
fn ident_before_trailing_semicolon(
    document: &ParsedDocument,
    byte_offset: usize,
) -> Option<Node<'_>> {
    let source = document.source.as_bytes();
    let rest_of_line = source[byte_offset..].iter().take_while(|&&b| b != b'\n');
    if rest_of_line.clone().any(|b| !b.is_ascii_whitespace()) {
        return None;
    }
    let root = document.tree.root_node();
    let semicolon = significant_node_before_byte(root, source, byte_offset)?;
    if semicolon.kind() != ";" {
        return None;
    }
    let before_semicolon = significant_node_before_byte(root, source, semicolon.start_byte())?;
    identifier_at(root, before_semicolon.end_byte().checked_sub(1)?)
}

pub(super) fn all_declarations_of(definition: &Definition, db: &SymbolDb) -> Vec<Definition> {
    let Some((container, name)) = logical_member(&definition.symbol) else {
        return vec![definition.clone()];
    };

    let mut decls = db.all_member_declarations(&container, &name);
    if !decls
        .iter()
        .any(|d| definition_key(d) == definition_key(definition))
    {
        decls.push(definition.clone());
    }
    dedup_definitions(decls)
}

pub(super) fn logical_member(symbol: &Symbol) -> Option<(String, String)> {
    match symbol.kind {
        SymbolKind::Field if symbol.container.is_none() => {
            annotation_target_class(symbol).map(|t| (t.to_string(), symbol.name.clone()))
        }
        SymbolKind::Method | SymbolKind::Field => symbol
            .container_name
            .as_deref()
            .map(|cn| (cn.to_string(), symbol.name.clone())),
        SymbolKind::Function if symbol.container.is_none() => {
            annotation_target_class(symbol).map(|t| (t.to_string(), symbol.name.clone()))
        }
        _ => None,
    }
}

pub(super) fn definition_key(definition: &Definition) -> (String, std::ops::Range<usize>) {
    (
        definition.uri.clone(),
        definition.symbol.selection_byte_range.clone(),
    )
}

pub(super) fn resolve_self_keyword(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<Definition> {
    let root = document.tree.root_node();
    let node = nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|n| {
            find_ancestor_of_kind(
                n,
                &[kinds::THIS_EXPR, kinds::SUPER_EXPR, kinds::PARENT_EXPR],
            )
        })?;

    let current_type = enclosing_type_context(document, db, byte_offset)?;
    let in_state = current_type.owner_class.is_some();
    let owner = current_type.owner_class.clone();
    match node.kind() {
        kinds::THIS_EXPR => {
            if in_state
                && let Some(owner_class) = owner.as_deref()
                && let Some(def) = db.find_state_in_owner_chain(owner_class, &current_type.name)
            {
                return Some(def);
            }
            resolve_document_top_level(uri, document, &current_type.name)
                .or_else(|| db.find_top_level(&current_type.name))
        }
        kinds::SUPER_EXPR => {
            let base_name = current_type.base_class.as_deref()?;
            if in_state
                && let Some(owner_class) = owner.as_deref()
                && let Some(def) = db.find_state_in_owner_chain(owner_class, base_name)
            {
                return Some(def);
            }
            resolve_document_top_level(uri, document, base_name)
                .or_else(|| db.find_top_level(base_name))
        }
        kinds::PARENT_EXPR => {
            let owner_name = current_type.owner_class.as_deref()?;
            resolve_document_top_level(uri, document, owner_name)
                .or_else(|| db.find_top_level(owner_name))
        }
        _ => None,
    }
}

pub(super) fn resolve_at_definition_site(
    uri: &str,
    document: &ParsedDocument,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    document
        .symbols
        .all()
        .iter()
        .find(|symbol| {
            symbol.name == name
                && symbol.selection_byte_range.start <= byte_offset
                && byte_offset < symbol.selection_byte_range.end
        })
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
}
