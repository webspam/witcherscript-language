use tree_sitter::Node;

use super::{FormatOptions, Formatter, LayoutDirective};

pub(super) fn node_indent_level(node: Node, options: &FormatOptions) -> usize {
    let col = node.start_position().column;
    if options.use_tabs {
        col
    } else {
        col / (options.tab_size.max(1) as usize)
    }
}

pub(super) fn formatter_for<'a>(
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
