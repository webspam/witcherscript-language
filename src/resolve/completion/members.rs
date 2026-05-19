use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::AccessLevel;

use super::super::ast::{find_ancestor_of_kind, first_named_child, nodes_at_offset};
use super::super::db::SymbolDb;
use super::super::inference::{enclosing_type_context, infer_expr_type};
use super::super::Definition;

pub fn completion_members(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<(u8, Definition)> {
    completion_members_inner(uri, document, db, position).unwrap_or_default()
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

    let root = document.tree.root_node();
    let access_node = nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|n| {
            find_ancestor_of_kind(n, &["member_access_expr", "incomplete_member_access_expr"])
        })?;

    let expr = first_named_child(access_node)?;
    let context_byte = expr.start_byte();

    let type_name = match expr.kind() {
        "super_expr" | "super" => {
            let current_type = enclosing_type_context(document, db, context_byte)?;
            current_type.base_class?
        }
        "parent_expr" | "parent" => {
            let current_type = enclosing_type_context(document, db, context_byte)?;
            current_type.owner_class?
        }
        _ => infer_expr_type(uri, document, db, expr, context_byte)?,
    };

    Some(db.members_of_tiered(&type_name, AccessLevel::Public))
}
