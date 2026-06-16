use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::grammar::write_target;
use crate::cst::nav::{first_child_kind, single_name};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::formatter::line_indent;
use crate::symbols::SymbolKind;

use super::Definition;
use super::ast::nodes_at_offset;
use super::body_model::{BodyModel, JoinTarget};
use super::definition::resolve_definition_at_byte;
use super::extract_common::{Splice, delete_statement};
use super::symbol_db::SymbolDb;

pub fn join_declaration(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte: usize,
) -> Option<Vec<Splice>> {
    let root = document.tree.root_node();
    let (def, from_assignment) = target_local(uri, document, db, root, byte)?;
    if def.symbol.kind != SymbolKind::Variable || def.uri.as_str() != uri {
        return None;
    }

    let anchor = def.symbol.selection_byte_range.start;
    let model = BodyModel::enclosing(uri, document, db, anchor)?;
    let local = model.local_declared_at(anchor)?;
    let JoinTarget {
        value,
        stmt,
        insert_at,
    } = model.joinable_assignment(local)?;

    // When the cursor is on an assignment, join that one rather than an earlier assignment.
    if from_assignment.is_some_and(|cursor_stmt| cursor_stmt != stmt) {
        return None;
    }

    let init = &document.source[value];
    Some(vec![
        Splice {
            range: insert_at..insert_at,
            text: format!(" = {init}"),
        },
        delete_statement(&document.source, stmt),
    ])
}

pub fn split_declaration(document: &ParsedDocument, byte: usize) -> Option<Vec<Splice>> {
    let root = document.tree.root_node();
    let decl = enclosing(root, byte, kinds::LOCAL_VAR_DECL_STMT)?;
    if decl.parent().map(|p| p.kind()) != Some(kinds::FUNC_BLOCK) {
        return None;
    }
    let name = single_name(decl)?;
    let init = decl.child_by_field_name(fields::INIT_VALUE)?;
    let var_type = decl.child_by_field_name(fields::VAR_TYPE)?;

    let source = &document.source;
    let assignment = format!(
        "\n{indent}{name} = {init};",
        indent = line_indent(source, decl.start_byte()),
        name = &source[name.byte_range()],
        init = &source[init.byte_range()],
    );
    let after = decl.end_byte();
    Some(vec![
        Splice {
            range: var_type.end_byte()..init.end_byte(),
            text: String::new(),
        },
        Splice {
            range: after..after,
            text: assignment,
        },
    ])
}

fn target_local(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    root: Node<'_>,
    byte: usize,
) -> Option<(Definition, Option<Range<usize>>)> {
    if let Some(decl) = enclosing(root, byte, kinds::LOCAL_VAR_DECL_STMT) {
        let name = single_name(decl)?;
        let def = resolve_definition_at_byte(uri, document, db, name.start_byte())?;
        return Some((def, None));
    }
    let stmt = enclosing(root, byte, kinds::EXPR_STMT)?;
    let assign = first_child_kind(stmt, kinds::ASSIGN_OP_EXPR)?;
    let target = write_target(assign.child_by_field_name(fields::LEFT)?)?;
    let def = resolve_definition_at_byte(uri, document, db, target.start_byte())?;
    Some((def, Some(stmt.byte_range())))
}

fn enclosing<'t>(root: Node<'t>, byte: usize, kind: &str) -> Option<Node<'t>> {
    nodes_at_offset(root, byte)
        .into_iter()
        .find_map(|n| find_ancestor_of_kind(n, &[kind]))
}
