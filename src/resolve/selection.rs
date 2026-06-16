use std::ops::Range;

use tree_sitter::Node;

use crate::cst::grammar::call_callee;
use crate::cst::{fields, kinds};

const EXTRACTABLE_KINDS: &[&str] = &[
    kinds::BINARY_OP_EXPR,
    kinds::UNARY_OP_EXPR,
    kinds::FUNC_CALL_EXPR,
    kinds::MEMBER_ACCESS_EXPR,
    kinds::ARRAY_EXPR,
    kinds::NESTED_EXPR,
    kinds::CAST_EXPR,
    kinds::NEW_EXPR,
    kinds::IDENT,
    kinds::LITERAL_INT,
    kinds::LITERAL_HEX,
    kinds::LITERAL_FLOAT,
    kinds::LITERAL_BOOL,
    kinds::LITERAL_STRING,
    kinds::LITERAL_NAME,
];

pub(super) fn trim_selection(source: &str, selection: Range<usize>) -> Option<Range<usize>> {
    let slice = source.get(selection.clone())?;
    let start = selection.start + (slice.len() - slice.trim_start().len());
    // A trailing `;` is not part of the value; selecting `x;` means the value `x`, not the statement.
    let trimmed = slice.trim_end_matches(|c: char| c.is_whitespace() || c == ';');
    let end = selection.end - (slice.len() - trimmed.len());
    (start < end).then_some(start..end)
}

// The smallest covering node can be a leaf inside same-range wrappers; keep the outermost extractable one.
fn exact_expression_at<'tree>(root: Node<'tree>, selection: &Range<usize>) -> Option<Node<'tree>> {
    let mut node = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    if node.byte_range() != *selection {
        return None;
    }
    let mut best = None;
    loop {
        if EXTRACTABLE_KINDS.contains(&node.kind()) {
            best = Some(node);
        }
        match node.parent() {
            Some(parent) if parent.byte_range() == *selection => node = parent,
            _ => return best,
        }
    }
}

// A selection landing on a structural boundary expands to the whole value rather than refusing.
fn expand_selection(root: Node, selection: &Range<usize>) -> Option<Range<usize>> {
    expand_through_logical_operator(root, selection)
        .or_else(|| expand_through_postfix_chain(root, selection))
        .or_else(|| expand_through_new_expr(root, selection))
}

// In `new T in obj`, only the lifetime object is a standalone value; the rest expands to the whole.
fn expand_through_new_expr(root: Node, selection: &Range<usize>) -> Option<Range<usize>> {
    let mut node = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    loop {
        if node.kind() == kinds::NEW_EXPR && !selection_within_lifetime_obj(node, selection) {
            return Some(node.byte_range());
        }
        node = node.parent()?;
    }
}

fn selection_within_lifetime_obj(new_expr: Node, selection: &Range<usize>) -> bool {
    new_expr
        .child_by_field_name(fields::LIFETIME_OBJ)
        .is_some_and(|obj| obj.start_byte() <= selection.start && selection.end <= obj.end_byte())
}

const POSTFIX_CHAIN_KINDS: &[&str] = &[
    kinds::MEMBER_ACCESS_EXPR,
    kinds::FUNC_CALL_EXPR,
    kinds::ARRAY_EXPR,
];

// Promoting a touched method reference to its call yields a value, not an uncallable handle.
fn expand_through_postfix_chain(root: Node, selection: &Range<usize>) -> Option<Range<usize>> {
    let mut node = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    loop {
        if POSTFIX_CHAIN_KINDS.contains(&node.kind())
            && selection_touches_separator(node, selection)
        {
            return Some(promote_callee(node).byte_range());
        }
        node = node.parent()?;
    }
}

fn selection_touches_separator(node: Node, selection: &Range<usize>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        !child.is_named()
            && child.start_byte() < selection.end
            && selection.start < child.end_byte()
    })
}

fn promote_callee(node: Node) -> Node {
    node.parent()
        .filter(|parent| parent.kind() == kinds::FUNC_CALL_EXPR)
        .filter(|parent| {
            parent
                .child_by_field_name(fields::FUNC)
                .is_some_and(|func| func.id() == node.id())
        })
        .unwrap_or(node)
}

// Extracting both operands the touched `||`/`&&` joins keeps short-circuit evaluation intact.
fn expand_through_logical_operator(root: Node, selection: &Range<usize>) -> Option<Range<usize>> {
    let mut node = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    loop {
        if node.kind() == kinds::BINARY_OP_EXPR && selection_touches_logical_op(node, selection) {
            return Some(node.byte_range());
        }
        node = node.parent()?;
    }
}

fn selection_touches_logical_op(binary: Node, selection: &Range<usize>) -> bool {
    let Some(op) = binary.child_by_field_name(fields::OP) else {
        return false;
    };
    matches!(op.kind(), kinds::BINARY_OP_OR | kinds::BINARY_OP_AND)
        && op.start_byte() < selection.end
        && selection.start < op.end_byte()
}

pub(super) fn is_call_callee(node: Node) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == kinds::FUNC_CALL_EXPR)
        .and_then(call_callee)
        .is_some_and(|callee| callee.id() == node.id())
}

pub(super) enum SelectionKind<'tree> {
    Expression {
        node: Node<'tree>,
        range: Range<usize>,
    },
    Statements {
        range: Range<usize>,
    },
}

pub(super) fn classify_selection<'tree>(
    root: Node<'tree>,
    selection: &Range<usize>,
) -> SelectionKind<'tree> {
    let expanded = expand_selection(root, selection).unwrap_or_else(|| selection.clone());
    let Some(node) = exact_expression_at(root, &expanded) else {
        return SelectionKind::Statements {
            range: selection.clone(),
        };
    };
    // An expression that is an entire statement is a statement, not a value to bind or return.
    match node.parent().filter(|p| p.kind() == kinds::EXPR_STMT) {
        Some(stmt) => SelectionKind::Statements {
            range: stmt.byte_range(),
        },
        None => SelectionKind::Expression {
            node,
            range: expanded,
        },
    }
}
