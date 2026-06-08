use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::offsets::nodes_at_offset;

use super::action::{formatter_for, indent_unit_for, node_indent_level, splice_subs, Substitution};
use super::statements::{block_single_stmt, body_expandable, chain_bodies};
use super::{collect_comments, FormatOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfLayout {
    Collapse,
    Expand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IfToggle {
    pub can_collapse: bool,
    pub can_expand: bool,
}

/// The if/else chain enclosing `byte` (climbed to its leading `if`), or `None`.
pub fn if_chain_at(root: Node, byte: usize) -> Option<Node> {
    nodes_at_offset(root, byte)
        .into_iter()
        .find_map(|n| find_ancestor_of_kind(n, &["if_stmt"]))
        .map(if_chain_head)
}

// An `else if` is parsed as the outer if's `else` child.
fn if_chain_head(mut node: Node) -> Node {
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
    let f = formatter_for(source, options, comments, level);
    f.if_toggle(if_node)
}

/// Verbatim structural rewrite of the if-chain at `if_node` to `layout`: braces are added or removed
/// and the moved statement is re-indented, but every other token is copied byte-for-byte. Reflowing
/// the conditions and bodies is the formatter's job, run separately if the user wants it.
pub fn rewrite_if_layout(
    if_node: Node,
    source: &str,
    options: FormatOptions,
    layout: IfLayout,
) -> String {
    let unit = indent_unit_for(&options);
    let level = node_indent_level(if_node, &options);
    let subs = chain_bodies(if_node)
        .into_iter()
        .filter_map(|body| body_substitution(body, source, layout, level, &unit))
        .collect();
    splice_subs(source, if_node.start_byte(), if_node.end_byte(), subs)
}

fn body_substitution(
    body: Node,
    source: &str,
    layout: IfLayout,
    level: usize,
    unit: &str,
) -> Option<Substitution> {
    let (start, end) = (body.start_byte(), body.end_byte());
    match layout {
        IfLayout::Collapse => {
            let inner = block_single_stmt(body)?;
            let text = source[inner.start_byte()..inner.end_byte()].to_string();
            Some(Substitution { start, end, text })
        }
        IfLayout::Expand => {
            if !body_expandable(body) {
                return None;
            }
            let stmt = &source[start..end];
            let inner = unit.repeat(level + 1);
            let close = unit.repeat(level);
            let text = format!("{{\n{inner}{stmt}\n{close}}}");
            Some(Substitution { start, end, text })
        }
    }
}
