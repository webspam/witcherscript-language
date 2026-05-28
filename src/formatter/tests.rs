use crate::document::parse_document;

mod alignment;
mod basic;
mod blank_lines;
mod calls;
mod colon_spacing;
mod comments;
mod line_breaking;
mod structures;

use super::AnnotationPlacement;

fn format_with(
    source: &str,
    compact_colon: bool,
    align_member_colons: bool,
    annotation_placement: AnnotationPlacement,
) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(
        doc.tree.root_node(),
        &doc.source,
        super::FormatOptions {
            tab_size: 4,
            use_tabs: false,
            line_limit: 100,
            compact_colon,
            align_member_colons,
            annotation_placement,
        },
    )
}

pub(super) fn fmt(source: &str) -> String {
    format_with(source, false, false, AnnotationPlacement::Preserve)
}

pub(super) fn fmt_compact_colon(source: &str) -> String {
    format_with(source, true, false, AnnotationPlacement::Preserve)
}

pub(super) fn fmt_aligned(source: &str) -> String {
    format_with(source, false, true, AnnotationPlacement::Preserve)
}

pub(super) fn fmt_annotation_own_line(source: &str) -> String {
    format_with(source, false, false, AnnotationPlacement::OwnLine)
}

pub(super) fn fmt_annotation_same_line(source: &str) -> String {
    format_with(source, false, false, AnnotationPlacement::SameLine)
}

pub(super) fn fmt_limit(source: &str, line_limit: u32) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(
        doc.tree.root_node(),
        &doc.source,
        super::FormatOptions {
            tab_size: 4,
            use_tabs: false,
            line_limit,
            compact_colon: false,
            align_member_colons: false,
            annotation_placement: AnnotationPlacement::Preserve,
        },
    )
}
