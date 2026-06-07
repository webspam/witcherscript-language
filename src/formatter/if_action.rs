use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::offsets::nodes_at_offset;

use super::action::{formatter_for, node_indent_level};
use super::{collect_comments, FormatOptions, LayoutDirective};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfLayout {
    Collapse,
    Expand,
}

impl From<IfLayout> for LayoutDirective {
    fn from(layout: IfLayout) -> Self {
        match layout {
            IfLayout::Collapse => LayoutDirective::IfCollapse,
            IfLayout::Expand => LayoutDirective::IfExpand,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IfToggle {
    pub can_collapse: bool,
    pub can_expand: bool,
}

/// Resolves to the outermost link, not the nearest `if_stmt`, so the action rewrites the whole chain.
pub fn if_stmt_on_keyword(root: Node, byte: usize) -> Option<Node> {
    nodes_at_offset(root, byte)
        .into_iter()
        .filter(|n| matches!(n.kind(), "if" | "else"))
        .find_map(|n| find_ancestor_of_kind(n, &["if_stmt"]))
        .map(outermost_if_chain)
}

// An `else if` is parsed as the outer if's `else` child.
fn outermost_if_chain(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        let is_else_link = parent.kind() == "if_stmt"
            && parent.child_by_field_name("else").map(|e| e.id()) == Some(node.id());
        if !is_else_link {
            break;
        }
        node = parent;
    }
    node
}

pub fn analyze_if(if_node: Node, source: &str, options: FormatOptions) -> IfToggle {
    let comments = collect_comments(if_node);
    let level = node_indent_level(if_node, &options);
    let f = formatter_for(source, options, comments, level, None);
    f.if_toggle(if_node)
}

pub fn format_if_with_layout(
    if_node: Node,
    source: &str,
    options: FormatOptions,
    layout: IfLayout,
) -> String {
    let comments = collect_comments(if_node);
    let level = node_indent_level(if_node, &options);
    let mut f = formatter_for(source, options, comments, level, Some(layout.into()));
    // The source indent before the node's start byte is kept, so skip the leading one.
    f.suppress_next_indent = true;
    f.format_if_stmt(if_node);
    let mut out = f.out;
    if out.ends_with('\n') {
        out.pop();
    }
    out
}
