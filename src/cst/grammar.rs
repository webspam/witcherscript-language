use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::nav::{first_child_kind, first_named_child};
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

/// `Empty` keeps the gap's byte offset (`f(a,,b)`) so a diagnostic can point at the missing arg.
pub(crate) enum ArgSlot<'tree> {
    Filled(Node<'tree>),
    Empty { gap: usize },
}

impl ArgSlot<'_> {
    pub(crate) fn is_filled(&self) -> bool {
        matches!(self, ArgSlot::Filled(_))
    }

    pub(crate) fn start_byte(&self) -> usize {
        match self {
            ArgSlot::Filled(n) => n.start_byte(),
            ArgSlot::Empty { gap } => *gap,
        }
    }

    pub(crate) fn end_byte(&self) -> usize {
        match self {
            ArgSlot::Filled(n) => n.end_byte(),
            ArgSlot::Empty { gap } => *gap,
        }
    }
}

/// One entry per positional slot; an empty `Vec` is a no-arg call `f()`.
pub(crate) fn arg_slots_with_gaps(call: Node) -> Vec<ArgSlot> {
    let Some(args) = call.child_by_field_name(fields::ARGS) else {
        return Vec::new();
    };
    let mut slots: Vec<ArgSlot> = Vec::new();
    let mut pending: Option<Node> = None;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        match child.kind() {
            "," => slots.push(close_slot(pending.take(), child.start_byte())),
            kinds::COMMENT => {}
            _ if child.is_named() => pending = Some(child),
            _ => {}
        }
    }
    slots.push(close_slot(pending.take(), args.end_byte()));
    slots
}

fn close_slot(pending: Option<Node>, gap: usize) -> ArgSlot {
    match pending {
        Some(node) => ArgSlot::Filled(node),
        None => ArgSlot::Empty { gap },
    }
}

/// The `)` that closes a call's argument list, or `None` if a parse error dropped it.
pub(crate) fn call_close_paren(call: Node) -> Option<Node> {
    first_child_kind(call, ")")
}

/// Argument slots of a call. `None` if no args or any slot is empty (`f(a,,b)`), which breaks positional alignment.
pub(crate) fn arg_slots(call: Node) -> Option<Vec<Node>> {
    let slots = arg_slots_with_gaps(call);
    if slots.is_empty() {
        return None;
    }
    slots
        .into_iter()
        .map(|slot| match slot {
            ArgSlot::Filled(node) => Some(node),
            ArgSlot::Empty { .. } => None,
        })
        .collect()
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
