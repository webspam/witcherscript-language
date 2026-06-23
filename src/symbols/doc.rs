use tree_sitter::Node;

use crate::cst::kinds;
use crate::line_index::LineIndex;

use super::util::node_text;

/// An `@` annotation is the declaration's first child, so the comment above the `@` is the one above `node`.
pub(super) fn doc_comment_above(
    node: Node,
    source: &str,
    line_index: &LineIndex,
) -> Option<String> {
    let mut comments = Vec::new();
    let mut lower_start = node.start_byte();
    let mut cur = node;
    while let Some(prev) = cur.prev_sibling() {
        if prev.kind() != kinds::COMMENT
            || !is_own_line_comment(prev, source, line_index)
            || blank_line_between(prev.end_byte(), lower_start, source)
        {
            break;
        }
        lower_start = prev.start_byte();
        cur = prev;
        comments.push(prev);
    }
    if comments.is_empty() {
        return None;
    }
    comments.reverse();
    let text = comments
        .iter()
        .map(|comment| clean_comment(&node_text(*comment, source)))
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn is_own_line_comment(comment: Node, source: &str, line_index: &LineIndex) -> bool {
    let row = comment.start_position().row;
    let Some(&line_start) = line_index.line_starts().get(row) else {
        return false;
    };
    source[line_start..comment.start_byte()]
        .chars()
        .all(char::is_whitespace)
}

fn blank_line_between(upper_end: usize, lower_start: usize, source: &str) -> bool {
    source
        .get(upper_end..lower_start)
        .is_some_and(|gap| gap.matches('\n').count() >= 2)
}

fn clean_comment(text: &str) -> String {
    let text = text.trim();
    if let Some(body) = text.strip_prefix("//") {
        let body = body.trim_start_matches('/');
        return body
            .strip_prefix(' ')
            .unwrap_or(body)
            .trim_end()
            .to_string();
    }
    let body = text
        .strip_prefix("/*")
        .map_or(text, |t| t.strip_suffix("*/").unwrap_or(t));
    body.lines()
        .map(|line| {
            let line = line.trim();
            line.strip_prefix('*').map_or(line, str::trim_start)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
