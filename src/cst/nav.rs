use tree_sitter::Node;

use crate::cst::{fields, kinds};

pub(crate) fn first_child_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    nth_child_kind(node, kind, 0)
}

pub(crate) fn nth_child_kind<'tree>(
    node: Node<'tree>,
    kind: &str,
    index: usize,
) -> Option<Node<'tree>> {
    let mut cursor = node.walk();

    node.children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .nth(index)
}

pub(crate) fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();

    node.named_children(&mut cursor).next()
}

pub(crate) fn child_nodes(node: Node) -> Vec<Node> {
    let mut c = node.walk();
    node.children(&mut c).collect()
}

pub(crate) fn named_child_nodes(node: Node) -> Vec<Node> {
    let mut c = node.walk();
    node.named_children(&mut c).collect()
}

pub(crate) fn decl_name_idents(decl: Node) -> Vec<Node> {
    let mut cursor = decl.walk();
    decl.children_by_field_name(fields::NAMES, &mut cursor)
        .filter(|n| n.kind() == kinds::IDENT)
        .collect()
}

pub(crate) fn single_name(decl: Node) -> Option<Node> {
    match decl_name_idents(decl).as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}
