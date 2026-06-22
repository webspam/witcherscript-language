use tree_sitter::Node;

use crate::cst::grammar::member_access_member;
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::line_index::{SourcePosition, SourceRange};
use crate::symbols::AccessLevel;

use super::super::Definition;
use super::super::ast::{find_ancestor_of_kind, first_named_child, significant_node_before_byte};
use super::super::inference::{enclosing_type_context, infer_type};
use super::super::symbol_db::SymbolDb;

pub fn completion_members(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<(u8, Definition)> {
    completion_members_inner(uri, document, db, position).unwrap_or_default()
}

// Anchor left of the cursor; a missing `;` glues the partial access onto the next statement.
fn member_access_node_at(document: &ParsedDocument, byte_offset: usize) -> Option<Node<'_>> {
    let root = document.tree.root_node();
    let anchor = significant_node_before_byte(root, document.source.as_bytes(), byte_offset)?;
    find_ancestor_of_kind(
        anchor,
        &[
            kinds::MEMBER_ACCESS_EXPR,
            kinds::INCOMPLETE_MEMBER_ACCESS_EXPR,
        ],
    )
}

// Member items anchor to this word so VS Code filters the name, not the leading `.` (vscode#14005).
pub fn member_completion_replace_range(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<SourceRange> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let access_node = member_access_node_at(document, byte_offset)?;
    let (start, end) = match member_access_member(access_node) {
        Some(member) => (member.start_byte(), member.end_byte()),
        None => (byte_offset, byte_offset),
    };
    Some(
        document
            .line_index
            .byte_range_to_range(&document.source, start, end),
    )
}

fn completion_members_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<(u8, Definition)>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let access_node = member_access_node_at(document, byte_offset)?;
    let expr = first_named_child(access_node)?;
    let context_byte = expr.start_byte();

    let type_name = match expr.kind() {
        kinds::SUPER_EXPR | "super" => {
            let current_type = enclosing_type_context(document, db, context_byte)?;
            current_type.base_class?
        }
        kinds::PARENT_EXPR | "parent" => {
            let current_type = enclosing_type_context(document, db, context_byte)?;
            current_type.owner_class?
        }
        _ => infer_type(uri, document, db, expr, context_byte).to_db_string()?,
    };

    Some(db.members_of_tiered(&type_name, AccessLevel::Public))
}
