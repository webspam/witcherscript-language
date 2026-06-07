use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::offsets::nodes_at_offset;

use super::{collect_comments, FormatOptions, Formatter, LayoutDirective};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchLayout {
    Collapse,
    Expand,
}

impl From<SwitchLayout> for LayoutDirective {
    fn from(layout: SwitchLayout) -> Self {
        match layout {
            SwitchLayout::Collapse => LayoutDirective::SwitchCollapse,
            SwitchLayout::Expand => LayoutDirective::SwitchExpand,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchToggle {
    pub can_collapse: bool,
    pub can_expand: bool,
}

/// The enclosing `switch_stmt` when `byte` sits on a `switch`/`case`/`default` keyword, else `None`.
pub fn switch_stmt_on_keyword(root: Node, byte: usize) -> Option<Node> {
    nodes_at_offset(root, byte)
        .into_iter()
        .filter(|n| matches!(n.kind(), "switch" | "case" | "default"))
        .find_map(|n| find_ancestor_of_kind(n, &["switch_stmt"]))
}

pub fn analyze_switch(switch_node: Node, source: &str, options: FormatOptions) -> SwitchToggle {
    let comments = collect_comments(switch_node);
    let level = switch_level(switch_node, &options);
    let f = formatter_for(source, options, comments, level, None);
    f.switch_toggle(switch_node)
}

/// Replacement text for the `switch_node`'s byte range with every arm forced to `layout`.
pub fn format_switch_with_layout(
    switch_node: Node,
    source: &str,
    options: FormatOptions,
    layout: SwitchLayout,
) -> String {
    let comments = collect_comments(switch_node);
    let level = switch_level(switch_node, &options);
    let mut f = formatter_for(source, options, comments, level, Some(layout.into()));
    // The source indent before the node's start byte is kept, so skip the leading one.
    f.suppress_next_indent = true;
    f.format_switch_stmt_impl(switch_node);
    let mut out = f.out;
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn switch_level(node: Node, options: &FormatOptions) -> usize {
    let col = node.start_position().column;
    if options.use_tabs {
        col
    } else {
        col / (options.tab_size.max(1) as usize)
    }
}

fn formatter_for<'a>(
    source: &'a str,
    options: FormatOptions,
    comments: Vec<Node<'a>>,
    level: usize,
    layout_directive: Option<LayoutDirective>,
) -> Formatter<'a> {
    let indent_unit = if options.use_tabs {
        "\t".to_string()
    } else {
        " ".repeat(options.tab_size as usize)
    };
    Formatter {
        source,
        indent_unit,
        level,
        out: String::new(),
        suppress_next_indent: false,
        line_limit: options.line_limit as usize,
        compact_colon: options.compact_colon,
        align_member_colons: options.align_member_colons,
        annotation_placement: options.annotation_placement,
        default_placement: options.default_placement,
        colon_align_col: None,
        comments,
        comment_cursor: 0,
        layout_directive,
    }
}
