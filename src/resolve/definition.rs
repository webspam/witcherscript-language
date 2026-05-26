use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{Symbol, SymbolKind};

use super::ast::{find_ancestor_of_kind, first_named_child, identifier_at, nodes_at_offset};
use super::inference::{
    enclosing_type_context, resolve_document_top_level, resolve_member_access, resolve_name,
    resolve_name_in_context,
};
use super::name_context::classify_ident_context;
use super::symbol_db::SymbolDb;
use super::{annotation_target_class, dedup_definitions, Definition};

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
    resolve_at_definition_site(uri, document, byte_offset, name)
        .or_else(|| resolve_for_ident_no_site_fallback(uri, document, db, ident, byte_offset))
}

fn resolve_for_ident_no_site_fallback(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    ident: Node,
    byte_offset: usize,
) -> Option<Definition> {
    let name = ident.utf8_text(document.source.as_bytes()).ok()?;

    if let Some(member_access) = ident.parent().filter(|p| p.kind() == "member_access_expr") {
        let is_receiver = first_named_child(member_access)
            .map(|r| r.id() == ident.id())
            .unwrap_or(false);
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
    let Some(primary) = resolve_definition(uri, document, db, position) else {
        return Vec::new();
    };
    all_declarations_of(&primary, db)
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
        .find_map(|n| find_ancestor_of_kind(n, &["this_expr", "super_expr", "parent_expr"]))?;

    let current_type = enclosing_type_context(document, db, byte_offset)?;
    let in_state = current_type.owner_class.is_some();
    let owner = current_type.owner_class.clone();
    match node.kind() {
        "this_expr" => {
            if in_state {
                if let Some(owner_class) = owner.as_deref() {
                    if let Some(def) = db.find_state_in_owner_chain(owner_class, &current_type.name)
                    {
                        return Some(def);
                    }
                }
            }
            resolve_document_top_level(uri, document, &current_type.name)
                .or_else(|| db.find_top_level(&current_type.name))
        }
        "super_expr" => {
            let base_name = current_type.base_class.as_deref()?;
            if in_state {
                if let Some(owner_class) = owner.as_deref() {
                    if let Some(def) = db.find_state_in_owner_chain(owner_class, base_name) {
                        return Some(def);
                    }
                }
            }
            resolve_document_top_level(uri, document, base_name)
                .or_else(|| db.find_top_level(base_name))
        }
        "parent_expr" => {
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
