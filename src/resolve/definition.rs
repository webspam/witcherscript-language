use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;

use super::ast::{first_named_child, identifier_at};
use super::db::SymbolDb;
use super::inference::{
    resolve_current_type_member, resolve_document_top_level, resolve_local_or_parameter,
    resolve_member_access,
};
use super::references::{
    definition_key, logical_member, resolve_at_definition_site, resolve_self_keyword,
};
use super::{dedup_definitions, Definition};

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
    resolve_for_ident_no_site_fallback(uri, document, db, ident, byte_offset).or_else(|| {
        let name = ident.utf8_text(document.source.as_bytes()).ok()?;
        resolve_at_definition_site(uri, document, byte_offset, name)
    })
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

    resolve_local_or_parameter(uri, document, byte_offset, name)
        .or_else(|| resolve_current_type_member(uri, document, db, byte_offset, name))
        .or_else(|| resolve_document_top_level(uri, document, name))
        .or_else(|| db.find_top_level(name))
        .or_else(|| db.find_enum_variant(name))
        .or_else(|| db.find_script_global(name))
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

    let Some((container, name)) = logical_member(&primary.symbol) else {
        return vec![primary];
    };

    let mut decls = db.all_member_declarations(&container, &name);
    if !decls
        .iter()
        .any(|d| definition_key(d) == definition_key(&primary))
    {
        decls.push(primary);
    }
    dedup_definitions(decls)
}
