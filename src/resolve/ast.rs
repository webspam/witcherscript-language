use tree_sitter::Node;

use crate::symbols::SymbolKind;

pub const BUILTIN_TYPES: &[&str] = &[
    "bool",
    "byte",
    "float",
    "int",
    "name",
    "string",
    "void",
    "Bool",
    "Float",
    "String",
    "CName",
    "Int32",
    "Int16",
    "Int8",
    "Uint8",
    "Uint16",
    "Uint32",
    "Uint64",
    "StringAnsi",
];

pub const BUILTIN_TYPE_COMPLETIONS: &[&str] =
    &["bool", "byte", "float", "int", "name", "string", "void"];

pub(super) fn find_ancestor_of_kind<'a>(mut node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    loop {
        if kinds.contains(&node.kind()) {
            return Some(node);
        }
        node = node.parent()?;
    }
}

pub(super) fn nearest_enclosing_block<'a>(mut node: Node<'a>) -> Option<Node<'a>> {
    const BLOCKS: &[&str] = &["func_block", "switch_block", "member_default_val_block"];
    loop {
        if BLOCKS.contains(&node.kind()) {
            return Some(node);
        }
        node = node.parent()?;
    }
}

pub(super) fn nodes_at_offset<'a>(root: Node<'a>, byte_offset: usize) -> Vec<Node<'a>> {
    let second = byte_offset.checked_sub(1);
    [Some(byte_offset), second]
        .into_iter()
        .flatten()
        .filter_map(|off| root.descendant_for_byte_range(off, off))
        .collect()
}

/// Nearest node before `byte_offset`, skipping whitespace and comments.
pub(super) fn significant_node_before_byte<'a>(
    root: Node<'a>,
    source: &[u8],
    byte_offset: usize,
) -> Option<Node<'a>> {
    let mut end = byte_offset;
    loop {
        let p = source[..end]
            .iter()
            .rposition(|&b| !b.is_ascii_whitespace())?;
        let node = root.descendant_for_byte_range(p, p + 1)?;
        if node.kind() != "comment" {
            return Some(node);
        }
        end = node.start_byte();
    }
}

pub(super) fn is_statement_boundary(node: Node) -> bool {
    if node.has_error() {
        return false;
    }
    if matches!(node.kind(), "{" | "}" | ";") {
        return true;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    // `)` closing an if condition without a curly-brace body is a statement boundary.
    let is_single_line_if = node.kind() == ")" && parent.kind() == "if_stmt";
    if is_single_line_if {
        return true;
    }
    if parent.kind() == "else_stmt" {
        return true;
    }
    // `:` closing a switch case/default label is also a statement boundary.
    node.kind() == ":" && matches!(parent.kind(), "switch_case_label" | "switch_default_label")
}

pub(super) fn is_type_annotation_boundary(node: Node) -> bool {
    if node.has_error() {
        return false;
    }
    node.kind() == ":"
        && !node.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                "switch_case_label" | "switch_default_label" | "ternary_cond_expr"
            )
        })
}

pub(super) fn is_kind_or_error_wrapped_kind(node: Node, kinds: &[&str]) -> bool {
    let effective = if node.is_error() && node.child_count() == 1 {
        node.child(0).unwrap()
    } else {
        node
    };
    kinds.contains(&effective.kind())
}

pub(super) fn identifier_at(root: Node, byte_offset: usize) -> Option<Node> {
    nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|node| {
            if node.kind() == "ident" {
                return Some(node);
            }
            let mut current = node;
            while let Some(parent) = current.parent() {
                if parent.kind() == "ident" {
                    return Some(parent);
                }
                current = parent;
            }
            None
        })
}

pub(super) fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let child = node.named_children(&mut cursor).next();
    child
}

pub(super) fn is_type_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
    )
}
