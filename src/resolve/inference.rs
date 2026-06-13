use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::grammar::{call_callee, member_access_member};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::{AccessLevel, Symbol, SymbolKind};
use crate::types::{Primitive, Type};

use super::ast::first_named_child;
use super::name_context::NameContext;
use super::symbol_db::SymbolDb;
use super::{Definition, annotation_target_class};

#[derive(Debug, Clone)]
pub(super) struct TypeContext {
    pub(super) name: String,
    pub(super) base_class: Option<String>,
    pub(super) owner_class: Option<String>,
}

pub(crate) fn infer_type_memo(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
    memo: &mut HashMap<(usize, usize), Type>,
) -> Type {
    let key = (node.start_byte(), node.end_byte());
    if let Some(cached) = memo.get(&key) {
        return cached.clone();
    }
    let value = infer_type(uri, document, db, node, context_byte);
    memo.insert(key, value.clone());
    value
}

/// Infer an expression's [`Type`]. [`Type::Unknown`] means not-confident; callers must treat it as "do not report".
pub(crate) fn infer_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
) -> Type {
    match node.kind() {
        kinds::IDENT => {
            let Ok(name) = node.utf8_text(document.source.as_bytes()) else {
                return Type::Unknown;
            };
            infer_name_type(uri, document, db, context_byte, name).unwrap_or(Type::Unknown)
        }
        kinds::FUNC_CALL_EXPR => match call_callee(node) {
            Some(func) => infer_type(uri, document, db, func, context_byte),
            None => Type::Unknown,
        },
        kinds::MEMBER_ACCESS_EXPR => {
            infer_member_access_type(uri, document, db, node, context_byte)
        }
        kinds::THIS_EXPR => named_or_unknown(current_type_name(document, db, context_byte)),
        kinds::LITERAL_INT | kinds::LITERAL_HEX => Type::Primitive(Primitive::Int),
        kinds::LITERAL_FLOAT => Type::Primitive(Primitive::Float),
        kinds::LITERAL_BOOL => Type::Primitive(Primitive::Bool),
        kinds::LITERAL_STRING => Type::Primitive(Primitive::String),
        kinds::LITERAL_NAME => Type::Primitive(Primitive::Name),
        kinds::LITERAL_NULL => Type::Null,
        kinds::NEW_EXPR => match node
            .child_by_field_name(fields::CLASS)
            .filter(|c| c.kind() == kinds::IDENT)
            .and_then(|c| c.utf8_text(document.source.as_bytes()).ok())
        {
            Some(name) => Type::from_annotation(name),
            None => Type::Unknown,
        },
        // A cast asserts the target type regardless of the inner value's type.
        kinds::CAST_EXPR => match node
            .child_by_field_name(fields::TYPE)
            .filter(|c| c.kind() == kinds::IDENT)
            .and_then(|c| c.utf8_text(document.source.as_bytes()).ok())
        {
            Some(name) => Type::from_annotation(name),
            None => Type::Unknown,
        },
        kinds::NESTED_EXPR => match first_named_child(node) {
            Some(inner) => infer_type(uri, document, db, inner, context_byte),
            None => Type::Unknown,
        },
        kinds::ARRAY_EXPR => match node.child_by_field_name(fields::ACCESSOR) {
            Some(accessor) => match infer_type(uri, document, db, accessor, context_byte) {
                Type::Array(elem) => *elem,
                _ => Type::Unknown,
            },
            None => Type::Unknown,
        },
        kinds::BINARY_OP_EXPR => infer_binary_op_type(uri, document, db, node, context_byte),
        kinds::UNARY_OP_EXPR => infer_unary_op_type(uri, document, db, node, context_byte),
        _ => Type::Unknown,
    }
}

fn infer_binary_op_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
) -> Type {
    let Some(op) = node.child_by_field_name(fields::OP) else {
        return Type::Unknown;
    };
    let operand = |field| match node.child_by_field_name(field) {
        Some(child) => infer_type(uri, document, db, child, context_byte),
        None => Type::Unknown,
    };
    match op.kind() {
        kinds::BINARY_OP_AND
        | kinds::BINARY_OP_OR
        | kinds::BINARY_OP_EQ
        | kinds::BINARY_OP_NEQ
        | kinds::BINARY_OP_GT
        | kinds::BINARY_OP_GE
        | kinds::BINARY_OP_LT
        | kinds::BINARY_OP_LE => Type::Primitive(Primitive::Bool),
        kinds::BINARY_OP_BITOR | kinds::BINARY_OP_BITAND | kinds::BINARY_OP_BITXOR => {
            Type::Primitive(Primitive::Int)
        }
        kinds::BINARY_OP_SUM => {
            let (left, right) = (operand(fields::LEFT), operand(fields::RIGHT));
            if is_concat_operand(&left) || is_concat_operand(&right) {
                Type::Primitive(Primitive::String)
            } else {
                arithmetic_join(&left, &right)
            }
        }
        kinds::BINARY_OP_DIFF
        | kinds::BINARY_OP_MULT
        | kinds::BINARY_OP_DIV
        | kinds::BINARY_OP_MOD => arithmetic_join(&operand(fields::LEFT), &operand(fields::RIGHT)),
        _ => Type::Unknown,
    }
}

fn infer_unary_op_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
) -> Type {
    let Some(op) = node.child_by_field_name(fields::OP) else {
        return Type::Unknown;
    };
    match op.kind() {
        kinds::UNARY_OP_NOT => Type::Primitive(Primitive::Bool),
        kinds::UNARY_OP_BITNOT => Type::Primitive(Primitive::Int),
        kinds::UNARY_OP_NEG | kinds::UNARY_OP_PLUS => match node.child_by_field_name(fields::RIGHT)
        {
            Some(operand) => infer_type(uri, document, db, operand, context_byte),
            None => Type::Unknown,
        },
        _ => Type::Unknown,
    }
}

// `+` on a string or name operand concatenates; name + name also yields string.
fn is_concat_operand(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(Primitive::String | Primitive::Name))
}

fn arithmetic_join(left: &Type, right: &Type) -> Type {
    match (left, right) {
        (Type::Primitive(l), Type::Primitive(r)) => numeric_widen(*l, *r),
        // A struct carries its type through arithmetic: Vector + Vector, Vector * float, float / Vector.
        (Type::Named(a), Type::Named(b)) if a == b => Type::Named(a.clone()),
        (Type::Named(n), Type::Primitive(p)) | (Type::Primitive(p), Type::Named(n))
            if is_numeric(*p) =>
        {
            Type::Named(n.clone())
        }
        _ => Type::Unknown,
    }
}

fn numeric_widen(l: Primitive, r: Primitive) -> Type {
    if !is_numeric(l) || !is_numeric(r) {
        return Type::Unknown;
    }
    if l == r {
        return Type::Primitive(l);
    }
    if l == Primitive::Float || r == Primitive::Float {
        return Type::Primitive(Primitive::Float);
    }
    if matches!(l, Primitive::Int | Primitive::Byte)
        && matches!(r, Primitive::Int | Primitive::Byte)
    {
        return Type::Primitive(Primitive::Int);
    }
    Type::Unknown
}

fn is_numeric(p: Primitive) -> bool {
    matches!(
        p,
        Primitive::Byte
            | Primitive::Int
            | Primitive::Int16
            | Primitive::Int8
            | Primitive::Uint16
            | Primitive::Uint32
            | Primitive::Uint64
            | Primitive::Float
    )
}

fn named_or_unknown(annotation: Option<String>) -> Type {
    annotation.map_or(Type::Unknown, |s| Type::from_annotation(&s))
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
    if member.kind() != kinds::IDENT {
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
    def.and_then(|d| d.symbol.type_annotation)
        .unwrap_or(Type::Unknown)
}

pub(super) fn resolve_member_access(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    ident: Node,
    name: &str,
) -> Option<Definition> {
    let parent = ident.parent()?;
    if parent.kind() != kinds::MEMBER_ACCESS_EXPR {
        return None;
    }

    let receiver = first_named_child(parent)?;
    match receiver.kind() {
        kinds::THIS_EXPR => {
            let current_type = current_type_name(document, db, ident.start_byte())?;
            resolve_document_member(uri, document, &current_type, name, AccessLevel::Private)
                .or_else(|| db.find_member(&current_type, name, AccessLevel::Private))
        }
        kinds::SUPER_EXPR | kinds::VIRTUAL_PARENT_EXPR => {
            let current_type = enclosing_type_context(document, db, ident.start_byte())?;
            db.find_member(
                current_type.base_class.as_deref()?,
                name,
                AccessLevel::Protected,
            )
        }
        kinds::PARENT_EXPR => {
            let current_type = enclosing_type_context(document, db, ident.start_byte())?;
            db.find_member(
                current_type.owner_class.as_deref()?,
                name,
                AccessLevel::Public,
            )
        }
        kinds::IDENT => {
            let receiver_name = receiver.utf8_text(document.source.as_bytes()).ok()?;
            let type_name = infer_name_type(uri, document, db, ident.start_byte(), receiver_name)?
                .to_db_string()?;
            resolve_document_member(uri, document, &type_name, name, AccessLevel::Public)
                .or_else(|| db.find_member(&type_name, name, AccessLevel::Public))
        }
        kinds::FUNC_CALL_EXPR | kinds::MEMBER_ACCESS_EXPR => {
            let type_name =
                infer_type(uri, document, db, receiver, ident.start_byte()).to_db_string()?;
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

pub(super) fn definition_type(definition: &Definition) -> Option<Type> {
    definition.symbol.type_annotation.clone().or_else(|| {
        if definition.symbol.kind == SymbolKind::EnumMember {
            definition
                .symbol
                .container_name
                .as_deref()
                .map(Type::from_annotation)
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
) -> Option<Type> {
    resolve_name_in_context(uri, document, db, byte_offset, name, &NameContext::Value)
        .and_then(|def| definition_type(&def))
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
