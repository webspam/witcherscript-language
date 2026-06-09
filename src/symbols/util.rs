use tree_sitter::Node;

use crate::cst::nav::{first_child_kind, nth_child_kind};

pub fn node_text(node: Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

pub(super) fn direct_child_text(node: Node, kind: &str, source: &str) -> Option<String> {
    first_child_kind(node, kind).map(|child| node_text(child, source))
}

pub(super) fn base_type(node: Node, source: &str) -> Option<String> {
    nth_child_kind(node, "ident", 1).map(|base| node_text(base, source))
}
