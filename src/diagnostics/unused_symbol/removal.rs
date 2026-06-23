use std::ops::Range;

use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::resolve::{delete_statement, remove_list_entry};

pub(super) fn separator(node: Node<'_>) -> Range<usize> {
    let prev = node
        .prev_sibling()
        .filter(|n| n.kind() == ",")
        .and_then(|comma| comma.prev_sibling())
        .map(|n| n.byte_range());
    let next = node
        .next_sibling()
        .filter(|n| n.kind() == ",")
        .and_then(|comma| comma.next_sibling())
        .map(|n| n.byte_range());
    remove_list_entry(&node.byte_range(), prev.as_ref(), next.as_ref()).range
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
