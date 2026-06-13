use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::node_and_ancestors;
use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::kinds;
use crate::symbols::SymbolId;

use super::ResolveCtx;

const LOOP_KINDS: &[&str] = &[kinds::FOR_STMT, kinds::WHILE_STMT, kinds::DO_WHILE_STMT];

pub(super) fn statement_run<'tree>(
    root: Node<'tree>,
    source: &str,
    selection: &Range<usize>,
) -> Option<(Node<'tree>, Vec<Node<'tree>>, Range<usize>)> {
    let probe = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    let block = node_and_ancestors(probe).find(|n| n.kind() == kinds::FUNC_BLOCK)?;
    let mut stmts = Vec::new();
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        let starts_within = selection.start <= child.start_byte();
        let content_covered = statement_content_end(child) <= selection.end;
        let overlaps = child.start_byte() < selection.end && selection.start < child.end_byte();
        if starts_within && content_covered {
            stmts.push(child);
        } else if overlaps {
            // A partially covered statement is ambiguous; expression mode handles sub-statement picks.
            return None;
        }
    }
    if stmts
        .iter()
        .all(|s| matches!(s.kind(), kinds::NOP | kinds::COMMENT))
    {
        return None;
    }
    let snapped = stmts.first()?.start_byte()..stmts.last()?.end_byte();
    if !source[selection.start..snapped.start].trim().is_empty() {
        return None;
    }
    // The selection may stop before the run's trailing `;`; only text reaching past it disqualifies.
    if selection.end > snapped.end && !source[snapped.end..selection.end].trim().is_empty() {
        return None;
    }
    Some((block, stmts, snapped))
}

// A trailing `;` is not content, so a selection that stops before it still covers the statement.
fn statement_content_end(stmt: Node) -> usize {
    let mut cursor = stmt.walk();
    stmt.named_children(&mut cursor)
        .last()
        .map_or(stmt.end_byte(), |child| child.end_byte())
}

pub(super) fn has_escaping_control_flow(stmts: &[Node], range: &Range<usize>) -> bool {
    let mut jumps = Vec::new();
    for stmt in stmts {
        if has_descendant_of_kind(*stmt, &[kinds::RETURN_STMT]) {
            return true;
        }
        collect_descendants_of_kind(
            *stmt,
            &[kinds::BREAK_STMT, kinds::CONTINUE_STMT],
            &mut jumps,
        );
    }
    jumps.iter().any(|jump| !jump_target_inside(*jump, range))
}

fn jump_target_inside(jump: Node, range: &Range<usize>) -> bool {
    node_and_ancestors(jump)
        .take_while(|n| range.start <= n.start_byte() && n.end_byte() <= range.end)
        .any(|n| {
            LOOP_KINDS.contains(&n.kind())
                || (jump.kind() == kinds::BREAK_STMT && n.kind() == kinds::SWITCH_STMT)
        })
}

pub(super) fn live_after(
    ctx: &ResolveCtx,
    callable_block: Node,
    first_stmt: Node,
    range: &Range<usize>,
    tracked: &HashSet<SymbolId>,
) -> HashSet<SymbolId> {
    if tracked.is_empty() {
        return HashSet::new();
    }
    let mut windows = Vec::with_capacity(2);
    windows.push(range.end..callable_block.end_byte());
    // A loop around the selection re-runs code that sits textually before it.
    if let Some(loop_node) = enclosing_loop(first_stmt, callable_block) {
        windows.push(loop_node.start_byte()..range.start);
    }
    let mut idents = Vec::new();
    collect_descendants_of_kind(callable_block, &[kinds::IDENT], &mut idents);
    idents
        .iter()
        .filter(|ident| windows.iter().any(|w| w.contains(&ident.start_byte())))
        .filter_map(|ident| ctx.resolve_at(ident.start_byte()))
        .filter(|def| def.uri == ctx.uri && tracked.contains(&def.symbol.id))
        .map(|def| def.symbol.id)
        .collect()
}

fn enclosing_loop<'tree>(node: Node<'tree>, stop: Node) -> Option<Node<'tree>> {
    node_and_ancestors(node)
        .take_while(|n| n.id() != stop.id())
        .filter(|n| LOOP_KINDS.contains(&n.kind()))
        .last()
}
