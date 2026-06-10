use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::nav::first_named_child;
use crate::cst::{fields, kinds};

// func_call_expr and member_access_expr tag their key children with grammar
// fields, but tree-sitter error recovery can drop the field tag while keeping
// the child - so each accessor falls back to the child's position.

pub(crate) fn call_callee(node: Node) -> Option<Node> {
    node.child_by_field_name(fields::FUNC)
        .or_else(|| first_named_child(node))
}

pub(crate) fn member_access_member(node: Node) -> Option<Node> {
    node.child_by_field_name(fields::MEMBER).or_else(|| {
        let mut cursor = node.walk();

        node.named_children(&mut cursor).nth(1)
    })
}

/// Argument slots of a call. `None` if no args or any slot is empty (`f(a,,b)`), which breaks positional alignment.
pub(crate) fn arg_slots(call: Node) -> Option<Vec<Node>> {
    let args = call.child_by_field_name(fields::ARGS)?;
    let mut slots: Vec<Option<Node>> = Vec::new();
    let mut pending: Option<Node> = None;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        match child.kind() {
            "," => slots.push(pending.take()),
            kinds::COMMENT => {}
            _ if child.is_named() => pending = Some(child),
            _ => {}
        }
    }
    slots.push(pending.take());
    slots.into_iter().collect()
}

pub(crate) fn callee_ident(callee: Node) -> Option<Node> {
    match callee.kind() {
        kinds::IDENT => Some(callee),
        kinds::MEMBER_ACCESS_EXPR | kinds::INCOMPLETE_MEMBER_ACCESS_EXPR => {
            member_access_member(callee).filter(|m| m.kind() == kinds::IDENT)
        }
        _ => None,
    }
}

pub(crate) const DEFAULT_OR_HINT_ASSIGN_KINDS: &[&str] = &[
    kinds::MEMBER_DEFAULT_VAL,
    kinds::MEMBER_DEFAULT_VAL_BLOCK_ASSIGN,
    kinds::MEMBER_HINT,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefaultOrHintKind {
    Default,
    Hint,
}

pub(crate) fn is_assignment_target(ident: Node) -> bool {
    let Some(assign) = find_ancestor_of_kind(ident, &[kinds::ASSIGN_OP_EXPR]) else {
        return false;
    };
    let Some(left) = assign.child_by_field_name(fields::LEFT) else {
        return false;
    };
    write_target(left).map(|n| n.id()) == Some(ident.id())
}

/// The terminal ident actually written by assigning to `expr`. For `a.b = x`
/// only `b` is written; for `a[i] = x` only `a`. `None` for non-assignable forms.
pub(crate) fn write_target(expr: Node) -> Option<Node> {
    match expr.kind() {
        kinds::IDENT => Some(expr),
        kinds::MEMBER_ACCESS_EXPR => write_target(member_access_member(expr)?),
        kinds::NESTED_EXPR => write_target(first_named_child(expr)?),
        kinds::ARRAY_EXPR => write_target(expr.child_by_field_name(fields::ACCESSOR)?),
        _ => None,
    }
}

pub(crate) fn ident_default_or_hint_kind(ident: Node) -> Option<DefaultOrHintKind> {
    let parent = ident.parent()?;
    let kind = match parent.kind() {
        kinds::MEMBER_DEFAULT_VAL | kinds::MEMBER_DEFAULT_VAL_BLOCK_ASSIGN => {
            DefaultOrHintKind::Default
        }
        kinds::MEMBER_HINT => DefaultOrHintKind::Hint,
        _ => return None,
    };
    (parent.child_by_field_name(fields::MEMBER).map(|n| n.id()) == Some(ident.id())).then_some(kind)
}
