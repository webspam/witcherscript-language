use std::ops::Range;

use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::resolve::delete_statement;

// Drop the adjacent comma too, else removing one name leaves a stray separator.
pub(super) fn separator(source: &str, node: Node<'_>) -> Range<usize> {
    if let Some(comma) = node.next_sibling().filter(|n| n.kind() == ",") {
        return node.start_byte()..consume_horizontal_ws(source, comma.end_byte());
    }
    if let Some(comma) = node.prev_sibling().filter(|n| n.kind() == ",") {
        return comma.start_byte()..node.end_byte();
    }
    node.byte_range()
}

pub(super) fn statement(source: &str, node: Node<'_>) -> Range<usize> {
    delete_statement(source, node.byte_range()).range
}

// A `default`/`hint` for a removed field would dangle, so delete those entries too.
pub(super) fn field_defaults(source: &str, field: Node<'_>, names: &[&str]) -> Vec<Range<usize>> {
    let Some(body) = field.parent() else {
        return Vec::new();
    };
    let mut cursor = body.walk();
    let mut ranges = Vec::new();
    for child in body.children(&mut cursor) {
        match child.kind() {
            kinds::MEMBER_DEFAULT_VAL | kinds::MEMBER_HINT => {
                push_if_targeted(&mut ranges, source, child, names);
            }
            kinds::MEMBER_DEFAULT_VAL_BLOCK => {
                let mut block_cursor = child.walk();
                for assign in child
                    .children(&mut block_cursor)
                    .filter(|a| a.kind() == kinds::MEMBER_DEFAULT_VAL_BLOCK_ASSIGN)
                {
                    push_if_targeted(&mut ranges, source, assign, names);
                }
            }
            _ => {}
        }
    }
    ranges
}

fn push_if_targeted(out: &mut Vec<Range<usize>>, source: &str, node: Node<'_>, names: &[&str]) {
    let Some(member) = node.child_by_field_name(fields::MEMBER) else {
        return;
    };
    let Ok(name) = member.utf8_text(source.as_bytes()) else {
        return;
    };
    if names.contains(&name) {
        out.push(delete_statement(source, node.byte_range()).range);
    }
}

fn consume_horizontal_ws(source: &str, mut end: usize) -> usize {
    let bytes = source.as_bytes();
    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t') {
        end += 1;
    }
    end
}
