use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::grammar::{call_callee, member_access_member};
use crate::document::ParsedDocument;
use crate::symbols::{AccessLevel, Symbol, SymbolKind};

use super::ast::first_named_child;
use super::symbol_db::SymbolDb;
use super::{annotation_target_class, Definition};

#[derive(Debug, Clone)]
pub(super) struct TypeContext {
    pub(super) name: String,
    pub(super) base_class: Option<String>,
    pub(super) owner_class: Option<String>,
}

pub fn infer_expr_type_memo(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
    memo: &mut HashMap<(usize, usize), Option<String>>,
) -> Option<String> {
    let key = (node.start_byte(), node.end_byte());
    if let Some(cached) = memo.get(&key) {
        return cached.clone();
    }
    let value = infer_expr_type(uri, document, db, node, context_byte);
    memo.insert(key, value.clone());
    value
}

pub(crate) fn infer_expr_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
) -> Option<String> {
    match node.kind() {
        "ident" => {
            let name = node.utf8_text(document.source.as_bytes()).ok()?;
            infer_name_type(uri, document, db, context_byte, name)
        }
        "func_call_expr" => {
            let func = call_callee(node)?;
            infer_expr_type(uri, document, db, func, context_byte)
        }
        "member_access_expr" => {
            let accessor = first_named_child(node)?;
            let member = member_access_member(node)?;
            if member.kind() != "ident" {
                return None;
            }
            let member_name = member.utf8_text(document.source.as_bytes()).ok()?;
            let container_type = infer_expr_type(uri, document, db, accessor, context_byte)?;
            let def = resolve_document_member(
                uri,
                document,
                &container_type,
                member_name,
                AccessLevel::Public,
            )
            .or_else(|| db.find_member(&container_type, member_name, AccessLevel::Public))?;
            def.symbol.type_annotation
        }
        "this_expr" => current_type_name(document, db, context_byte),
        _ => None,
    }
}

pub(super) fn resolve_member_access(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    ident: Node,
    name: &str,
) -> Option<Definition> {
    let parent = ident.parent()?;
    if parent.kind() != "member_access_expr" {
        return None;
    }

    let receiver = first_named_child(parent)?;
    match receiver.kind() {
        "this_expr" => {
            let current_type = current_type_name(document, db, ident.start_byte())?;
            resolve_document_member(uri, document, &current_type, name, AccessLevel::Private)
                .or_else(|| db.find_member(&current_type, name, AccessLevel::Private))
        }
        "super_expr" | "virtual_parent_expr" => {
            let current_type = enclosing_type_context(document, db, ident.start_byte())?;
            db.find_member(
                current_type.base_class.as_deref()?,
                name,
                AccessLevel::Protected,
            )
        }
        "parent_expr" => {
            let current_type = enclosing_type_context(document, db, ident.start_byte())?;
            db.find_member(
                current_type.owner_class.as_deref()?,
                name,
                AccessLevel::Public,
            )
        }
        "ident" => {
            let receiver_name = receiver.utf8_text(document.source.as_bytes()).ok()?;
            let type_name = infer_name_type(uri, document, db, ident.start_byte(), receiver_name)?;
            resolve_document_member(uri, document, &type_name, name, AccessLevel::Public)
                .or_else(|| db.find_member(&type_name, name, AccessLevel::Public))
        }
        "func_call_expr" | "member_access_expr" => {
            let type_name = infer_expr_type(uri, document, db, receiver, ident.start_byte())?;
            resolve_document_member(uri, document, &type_name, name, AccessLevel::Public)
                .or_else(|| db.find_member(&type_name, name, AccessLevel::Public))
        }
        _ => None,
    }
}

pub(super) fn resolve_name_local_to_workspace(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    resolve_local_or_parameter(uri, document, byte_offset, name)
        .or_else(|| resolve_current_type_member(uri, document, db, byte_offset, name))
        .or_else(|| resolve_document_top_level(uri, document, name))
        .or_else(|| db.find_top_level(name))
        .or_else(|| db.find_enum_variant(name))
}

pub(super) fn resolve_name(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    resolve_name_local_to_workspace(uri, document, db, byte_offset, name)
        .or_else(|| db.find_script_global(name))
}

pub(super) fn definition_type_name(definition: &Definition) -> Option<String> {
    definition.symbol.type_annotation.clone().or_else(|| {
        if definition.symbol.kind == SymbolKind::EnumVariant {
            definition.symbol.container_name.clone()
        } else {
            None
        }
    })
}

pub(super) fn infer_name_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
    name: &str,
) -> Option<String> {
    resolve_name_local_to_workspace(uri, document, db, byte_offset, name)
        .and_then(|def| definition_type_name(&def))
        .or_else(|| db.script_global_type(name))
}

pub(super) fn resolve_local_or_parameter(
    uri: &str,
    document: &ParsedDocument,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    let callable = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    )?;
    let symbol = document
        .symbols
        .local_at_byte(callable.id, name, byte_offset)?
        .clone();
    Some(Definition {
        uri: uri.to_string(),
        symbol,
    })
}

pub(super) fn resolve_current_type_member(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    let current_type = current_type_name(document, db, byte_offset)?;
    resolve_document_member(uri, document, &current_type, name, AccessLevel::Private)
        .or_else(|| db.find_member(&current_type, name, AccessLevel::Private))
}

fn resolve_document_member(
    uri: &str,
    document: &ParsedDocument,
    container_name: &str,
    name: &str,
    min_access: AccessLevel,
) -> Option<Definition> {
    let container = document.symbols.type_by_name(container_name)?;
    let symbol = document
        .symbols
        .member_of(container.id, name)
        .find(|s| s.access >= min_access)?
        .clone();
    Some(Definition {
        uri: uri.to_string(),
        symbol,
    })
}

pub(super) fn resolve_document_top_level(
    uri: &str,
    document: &ParsedDocument,
    name: &str,
) -> Option<Definition> {
    let symbol = document.symbols.top_level_by_name(name)?.clone();
    Some(Definition {
        uri: uri.to_string(),
        symbol,
    })
}

fn current_type_name(
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<String> {
    enclosing_type_context(document, db, byte_offset).map(|ctx| ctx.name)
}

fn current_type_symbol(document: &ParsedDocument, byte_offset: usize) -> Option<&Symbol> {
    document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
    )
}

/// Falls back to the annotation target when not syntactically inside a type.
pub(super) fn enclosing_type_context(
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<TypeContext> {
    if let Some(symbol) = current_type_symbol(document, byte_offset) {
        return Some(TypeContext {
            name: symbol.name.clone(),
            base_class: db.superclass_of(&symbol.name),
            owner_class: symbol.owner_class.clone(),
        });
    }

    let callable = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    )?;
    if callable.container.is_some() || callable.kind != SymbolKind::Function {
        return None;
    }
    let target = annotation_target_class(callable)?;
    Some(TypeContext {
        name: target.to_string(),
        base_class: db.superclass_of(target),
        owner_class: db
            .find_top_level(target)
            .and_then(|def| def.symbol.owner_class.clone()),
    })
}
