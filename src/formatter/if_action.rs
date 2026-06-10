use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::offsets::nodes_at_offset;
use crate::cst::{fields, kinds};

use super::FormatOptions;
use super::action::{Substitution, indent_unit_for, layout_ctx, line_indent, splice_subs};
use super::statements::{block_single_stmt, body_expandable, chain_bodies};

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
        .find_map(|n| find_ancestor_of_kind(n, &[kinds::IF_STMT]))
        .map(if_chain_head)
}

// An `else if` is parsed as the outer if's `else` child.
fn if_chain_head(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        let is_else_link = parent.kind() == kinds::IF_STMT
            && parent.child_by_field_name(fields::ELSE).map(|e| e.id()) == Some(node.id());
        if !is_else_link {
            break;
        }
        node = parent;
    }
    node
}

pub fn analyze_if(if_node: Node, options: FormatOptions) -> IfToggle {
    layout_ctx(if_node, &options).if_toggle(if_node)
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
    let base = line_indent(source, if_node);
    let subs = chain_bodies(if_node)
        .into_iter()
        .filter_map(|body| body_substitution(body, source, layout, base, &unit))
        .collect();
    splice_subs(source, if_node.start_byte(), if_node.end_byte(), subs)
}

fn body_substitution(
    body: Node,
    source: &str,
    layout: IfLayout,
    base: &str,
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
            let text = format!("{{\n{base}{unit}{stmt}\n{base}}}");
            Some(Substitution { start, end, text })
        }
    }
}
