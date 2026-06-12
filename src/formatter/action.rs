use tree_sitter::Node;

use super::{FormatOptions, collect_comments};

pub(super) fn node_indent_level(node: Node, options: &FormatOptions) -> usize {
    let col = node.start_position().column;
    if options.use_tabs {
        col
    } else {
        col / (options.tab_size.max(1) as usize)
    }
}

pub(crate) fn indent_unit_for(options: &FormatOptions) -> String {
    if options.use_tabs {
        "\t".to_string()
    } else {
        " ".repeat(options.tab_size as usize)
    }
}

pub(crate) fn indent_block(block: &str, options: &FormatOptions) -> String {
    let indent = indent_unit_for(options);
    let mut out = String::with_capacity(block.len());

    for (i, line) in block.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !line.is_empty() {
            out.push_str(&indent);
            out.push_str(line);
        }
    }
    out
}

pub(crate) fn line_indent(source: &str, byte: usize) -> &str {
    let line_start = source[..byte].rfind('\n').map_or(0, |nl| nl + 1);
    let prefix = &source[line_start..byte];
    let ws_len = prefix
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(prefix.len());
    &prefix[..ws_len]
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

// The formatter state a layout toggle reads, without a full `Formatter`'s rendering machinery.
pub(super) struct LayoutCtx<'t> {
    pub(super) level: usize,
    pub(super) indent_width: usize,
    pub(super) line_limit: usize,
    pub(super) comments: Vec<Node<'t>>,
}

pub(super) fn layout_ctx<'t>(node: Node<'t>, options: &FormatOptions) -> LayoutCtx<'t> {
    LayoutCtx {
        level: node_indent_level(node, options),
        indent_width: indent_unit_for(options).len(),
        line_limit: options.line_limit as usize,
        comments: collect_comments(node),
    }
}
