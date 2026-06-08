use tree_sitter::Node;

use super::{FormatOptions, Formatter};

pub(super) fn node_indent_level(node: Node, options: &FormatOptions) -> usize {
    let col = node.start_position().column;
    if options.use_tabs {
        col
    } else {
        col / (options.tab_size.max(1) as usize)
    }
}

pub(super) fn indent_unit_for(options: &FormatOptions) -> String {
    if options.use_tabs {
        "\t".to_string()
    } else {
        " ".repeat(options.tab_size as usize)
    }
}

// One verbatim byte-range replacement inside the node a refactor is rewriting.
pub(super) struct Substitution {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Copy `source[start..end]`, replacing each substitution's byte range with its text. Subs must be
/// non-overlapping and contained in `[start, end]`; everything outside them is preserved verbatim.
pub(super) fn splice_subs(
    source: &str,
    start: usize,
    end: usize,
    mut subs: Vec<Substitution>,
) -> String {
    subs.sort_by_key(|s| s.start);
    let mut out = String::new();
    let mut cursor = start;
    for sub in subs {
        out.push_str(&source[cursor..sub.start]);
        out.push_str(&sub.text);
        cursor = sub.end;
    }
    out.push_str(&source[cursor..end]);
    out
}

pub(super) fn formatter_for<'a>(
    source: &'a str,
    options: FormatOptions,
    comments: Vec<Node<'a>>,
    level: usize,
) -> Formatter<'a> {
    Formatter {
        source,
        indent_unit: indent_unit_for(&options),
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
    }
}
