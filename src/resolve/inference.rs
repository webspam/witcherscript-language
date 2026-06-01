use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::grammar::{call_callee, member_access_member};
use crate::document::ParsedDocument;
use crate::symbols::{AccessLevel, Symbol, SymbolKind};
use crate::types::{Primitive, Type};

use super::ast::first_named_child;
use super::name_context::NameContext;
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

/// Adapter over [`infer_type`]: `None` == not confidently known (the legacy `String`-keyed contract).
pub(crate) fn infer_expr_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
) -> Option<String> {
    match infer_type(uri, document, db, node, context_byte) {
        Type::Unknown | Type::Null => None,
        t => t.to_db_string(),
    }
}

/// Infer an expression's [`Type`]. [`Type::Unknown`] means not-confident; callers must treat it as "do not report".
pub fn infer_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
) -> Type {
    match node.kind() {
        "ident" => {
            let Ok(name) = node.utf8_text(document.source.as_bytes()) else {
                return Type::Unknown;
            };
            named_or_unknown(infer_name_type(uri, document, db, context_byte, name))
        }
        "func_call_expr" => match call_callee(node) {
            Some(func) => infer_type(uri, document, db, func, context_byte),
            None => Type::Unknown,
        },
        "member_access_expr" => infer_member_access_type(uri, document, db, node, context_byte),
        "this_expr" => named_or_unknown(current_type_name(document, db, context_byte)),
        "literal_int" | "literal_hex" => Type::Primitive(Primitive::Int),
        "literal_float" => Type::Primitive(Primitive::Float),
        "literal_bool" => Type::Primitive(Primitive::Bool),
        "literal_string" => Type::Primitive(Primitive::String),
        "literal_name" => Type::Primitive(Primitive::Name),
        "literal_null" => Type::Null,
        "new_expr" => match node
            .child_by_field_name("class")
            .filter(|c| c.kind() == "ident")
            .and_then(|c| c.utf8_text(document.source.as_bytes()).ok())
        {
            Some(name) => Type::from_annotation(name),
            None => Type::Unknown,
        },
        // A cast asserts the target type regardless of the inner value's type.
        "cast_expr" => match node
            .child_by_field_name("type")
            .filter(|c| c.kind() == "ident")
            .and_then(|c| c.utf8_text(document.source.as_bytes()).ok())
        {
            Some(name) => Type::from_annotation(name),
            None => Type::Unknown,
        },
        "nested_expr" => match first_named_child(node) {
            Some(inner) => infer_type(uri, document, db, inner, context_byte),
            None => Type::Unknown,
        },
        "array_expr" => match node.child_by_field_name("accessor") {
            Some(accessor) => match infer_type(uri, document, db, accessor, context_byte) {
                Type::Array(elem) => *elem,
                _ => Type::Unknown,
            },
            None => Type::Unknown,
        },
        _ => Type::Unknown,
    }
}

fn named_or_unknown(annotation: Option<String>) -> Type {
    annotation
        .map(|s| Type::from_annotation(&s))
        .unwrap_or(Type::Unknown)
}

fn infer_member_access_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
) -> Type {
    let Some(accessor) = first_named_child(node) else {
        return Type::Unknown;
    };
    let Some(member) = member_access_member(node) else {
        return Type::Unknown;
    };
    if member.kind() != "ident" {
        return Type::Unknown;
    }
    let Ok(member_name) = member.utf8_text(document.source.as_bytes()) else {
        return Type::Unknown;
    };
    let Some(container_type) = infer_type(uri, document, db, accessor, context_byte).to_db_string()
    else {
        return Type::Unknown;
    };
    let def = resolve_document_member(
        uri,
        document,
        &container_type,
        member_name,
        AccessLevel::Public,
    )
    .or_else(|| db.find_member(&container_type, member_name, AccessLevel::Public));
    named_or_unknown(def.and_then(|d| d.symbol.type_annotation))
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
        .or_else(|| db.find_enum_member(name))
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

pub(super) fn resolve_name_in_context(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
    name: &str,
    ctx: &NameContext,
) -> Option<Definition> {
    match ctx {
        NameContext::Type => resolve_document_top_level_filtered(uri, document, name, ctx)
            .or_else(|| db.find_top_level_filtered(name, ctx)),
        NameContext::StateExtends { owner_class } => {
            db.find_state_in_owner_chain(owner_class, name)
        }
        NameContext::Callable => resolve_local_or_parameter(uri, document, byte_offset, name)
            .or_else(|| resolve_current_type_member(uri, document, db, byte_offset, name))
            .or_else(|| resolve_document_top_level_filtered(uri, document, name, ctx))
            .or_else(|| db.find_top_level_filtered(name, ctx))
            .or_else(|| db.find_script_global(name)),
        NameContext::Value => resolve_local_or_parameter(uri, document, byte_offset, name)
            .or_else(|| resolve_current_type_member(uri, document, db, byte_offset, name))
            .or_else(|| resolve_document_top_level_filtered(uri, document, name, ctx))
            .or_else(|| db.find_top_level_filtered(name, ctx))
            .or_else(|| db.find_enum_member(name))
            .or_else(|| db.find_script_global(name)),
    }
}

fn resolve_document_top_level_filtered(
    uri: &str,
    document: &ParsedDocument,
    name: &str,
    ctx: &NameContext,
) -> Option<Definition> {
    let symbol = document
        .symbols
        .top_level_by_name_filtered(name, |k| ctx.accepts(k))?
        .clone();
    Some(Definition {
        uri: uri.to_string(),
        symbol,
    })
}

pub(super) fn definition_type_name(definition: &Definition) -> Option<String> {
    definition.symbol.type_annotation.clone().or_else(|| {
        if definition.symbol.kind == SymbolKind::EnumMember {
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
    resolve_name_in_context(uri, document, db, byte_offset, name, &NameContext::Value)
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
