use crate::document::parse_document;

mod alignment;
mod basic;
mod blank_lines;
mod calls;
mod colon_spacing;
mod comments;
mod line_breaking;
mod structures;

pub(super) fn fmt(source: &str) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(
        doc.tree.root_node(),
        &doc.source,
        4,
        false,
        100,
        false,
        false,
    )
}

pub(super) fn fmt_compact_colon(source: &str) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(
        doc.tree.root_node(),
        &doc.source,
        4,
        false,
        100,
        true,
        false,
    )
}

pub(super) fn fmt_aligned(source: &str) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(
        doc.tree.root_node(),
        &doc.source,
        4,
        false,
        100,
        false,
        true,
    )
}

pub(super) fn fmt_limit(source: &str, line_limit: u32) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(
        doc.tree.root_node(),
        &doc.source,
        4,
        false,
        line_limit,
        false,
        false,
    )
}
