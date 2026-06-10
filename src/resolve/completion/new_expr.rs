use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{AccessLevel, SymbolKind};

use super::super::Definition;
use super::super::ast::{nodes_at_offset, significant_node_before_byte};
use super::super::inference::{enclosing_type_context, infer_type};
use super::super::symbol_db::SymbolDb;
use crate::types::Type;

pub fn new_type_completions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    new_type_completions_inner(uri, document, db, position).unwrap_or_default()
}

fn new_type_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<Definition>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let root = document.tree.root_node();
    let source = document.source.as_bytes();

    let new_expr = at_new_class_slot(root, source, byte_offset)?;

    let mut types: Vec<Definition> = db
        .all_types()
        .into_iter()
        .filter(|def| def.symbol.kind == SymbolKind::Class)
        .collect();

    if let Some(expected) = expected_type_for_new(uri, document, db, new_expr, byte_offset)
        && db.find_top_level(&expected).is_some()
    {
        types.retain(|def| {
            def.symbol.name == expected || db.inherits_from(&def.symbol.name, &expected)
        });
    }
    Some(types)
}

pub fn new_lifetime_completions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    new_lifetime_completions_inner(uri, document, db, position).unwrap_or_default()
}

fn new_lifetime_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<Definition>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let root = document.tree.root_node();
    let source = document.source.as_bytes();

    if !at_new_lifetime_slot(root, source, byte_offset) {
        return None;
    }

    let callable = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    )?;

    let locals: Vec<Definition> = document
        .symbols
        .children_of(Some(callable.id))
        .filter(|sym| {
            matches!(sym.kind, SymbolKind::Variable | SymbolKind::Parameter)
                && sym.selection_byte_range.start <= byte_offset
        })
        .filter(|sym| is_class_typed(sym.type_annotation.as_ref(), db))
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
        .collect();

    let members: Vec<Definition> = enclosing_type_context(document, db, byte_offset)
        .map(|t| db.members_of(&t.name, AccessLevel::Private))
        .unwrap_or_default()
        .into_iter()
        .filter(|def| def.symbol.kind == SymbolKind::Field)
        .filter(|def| is_class_typed(def.symbol.type_annotation.as_ref(), db))
        .collect();

    let mut out = locals;
    out.extend(members);
    Some(out)
}

fn is_class_typed(type_annotation: Option<&Type>, db: &SymbolDb) -> bool {
    let Some(lookup) = type_annotation.and_then(Type::to_lookup_ctor) else {
        return false;
    };
    db.find_top_level(&lookup)
        .is_some_and(|def| def.symbol.kind == SymbolKind::Class)
}

// Falls back to the `new` keyword node when tree-sitter only recovered the
// keyword (e.g. `new ;` at statement level produces no `new_expr`).
fn at_new_class_slot<'a>(root: Node<'a>, source: &[u8], byte_offset: usize) -> Option<Node<'a>> {
    let prev = effective_prev_node(root, source, byte_offset)?;
    if prev.kind() != "new" {
        return None;
    }
    Some(find_ancestor_of_kind(prev, &[kinds::NEW_EXPR]).unwrap_or(prev))
}

// `new C in ;` detaches `in` into an ERROR sibling of new_expr; also accept that.
fn at_new_lifetime_slot(root: Node, source: &[u8], byte_offset: usize) -> bool {
    let Some(prev) = effective_prev_node(root, source, byte_offset) else {
        return false;
    };
    if prev.kind() != "in" {
        return false;
    }
    if find_ancestor_of_kind(prev, &[kinds::NEW_EXPR]).is_some() {
        return true;
    }
    significant_node_before_byte(root, source, prev.start_byte())
        .and_then(|n| find_ancestor_of_kind(n, &[kinds::NEW_EXPR]))
        .is_some()
}

fn effective_prev_node<'a>(root: Node<'a>, source: &[u8], byte_offset: usize) -> Option<Node<'a>> {
    nodes_at_offset(root, byte_offset)
        .last()
        .filter(|n| n.kind() == kinds::IDENT)
        .and_then(|n| significant_node_before_byte(root, source, n.start_byte()))
        .or_else(|| significant_node_before_byte(root, source, byte_offset))
}

fn expected_type_for_new(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    new_expr: Node,
    byte_offset: usize,
) -> Option<String> {
    let mut cur = new_expr;
    while let Some(parent) = cur.parent() {
        match parent.kind() {
            kinds::LOCAL_VAR_DECL_STMT | kinds::MEMBER_VAR_DECL => {
                let text = type_annot_text(parent, &document.source)?;
                return Type::from_annotation(&text).to_db_string();
            }
            kinds::ASSIGN_OP_EXPR => {
                let lhs = parent.child_by_field_name(fields::LEFT)?;
                return infer_type(uri, document, db, lhs, byte_offset).to_db_string();
            }
            kinds::FUNC_CALL_EXPR | kinds::FUNC_CALL_ARGS | kinds::FUNC_BLOCK | kinds::SCRIPT => {
                return None;
            }
            _ => cur = parent,
        }
    }
    None
}

fn type_annot_text(parent: Node, source: &str) -> Option<String> {
    let annot = parent.child_by_field_name(fields::VAR_TYPE)?;
    Some(source[annot.start_byte()..annot.end_byte()].to_string())
}
