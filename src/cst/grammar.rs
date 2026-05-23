use tree_sitter::Node;

use crate::cst::nav::first_named_child;

// func_call_expr and member_access_expr tag their key children with grammar
// fields, but tree-sitter error recovery can drop the field tag while keeping
// the child - so each accessor falls back to the child's position.

pub(crate) fn call_callee(node: Node) -> Option<Node> {
    node.child_by_field_name("func")
        .or_else(|| first_named_child(node))
}

pub(crate) fn member_access_member(node: Node) -> Option<Node> {
    node.child_by_field_name("member").or_else(|| {
        let mut cursor = node.walk();
        let member = node.named_children(&mut cursor).nth(1);
        member
    })
}

pub(crate) const DEFAULT_OR_HINT_ASSIGN_KINDS: &[&str] = &[
    "member_default_val",
    "member_default_val_block_assign",
    "member_hint",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefaultOrHintKind {
    Default,
    Hint,
}

pub(crate) fn ident_default_or_hint_kind(ident: Node) -> Option<DefaultOrHintKind> {
    let parent = ident.parent()?;
    let kind = match parent.kind() {
        "member_default_val" | "member_default_val_block_assign" => DefaultOrHintKind::Default,
        "member_hint" => DefaultOrHintKind::Hint,
        _ => return None,
    };
    (parent.child_by_field_name("member").map(|n| n.id()) == Some(ident.id())).then_some(kind)
}
