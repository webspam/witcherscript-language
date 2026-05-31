use crate::document::parse_document;

mod alignment;
mod basic;
mod blank_lines;
mod calls;
mod colon_spacing;
mod comments;
mod line_breaking;
mod structures;
mod switch;

use super::{AnnotationPlacement, FormatOptions};

fn fmt_options(source: &str, options: FormatOptions) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(doc.tree.root_node(), &doc.source, options)
}

pub(super) fn fmt(source: &str) -> String {
    fmt_options(source, FormatOptions::default())
}

pub(super) fn fmt_compact_colon(source: &str) -> String {
    fmt_options(
        source,
        FormatOptions {
            compact_colon: true,
            ..Default::default()
        },
    )
}

pub(super) fn fmt_aligned(source: &str) -> String {
    fmt_options(
        source,
        FormatOptions {
            align_member_colons: true,
            ..Default::default()
        },
    )
}

pub(super) fn fmt_with_annotation_placement(
    source: &str,
    placement: AnnotationPlacement,
) -> String {
    fmt_options(
        source,
        FormatOptions {
            annotation_placement: placement,
            ..Default::default()
        },
    )
}

pub(super) fn fmt_with_default_placement(source: &str, placement: AnnotationPlacement) -> String {
    fmt_options(
        source,
        FormatOptions {
            default_placement: placement,
            ..Default::default()
        },
    )
}

pub(super) fn fmt_aligned_with_default_placement(
    source: &str,
    placement: AnnotationPlacement,
) -> String {
    fmt_options(
        source,
        FormatOptions {
            align_member_colons: true,
            default_placement: placement,
            ..Default::default()
        },
    )
}

pub(super) fn fmt_limit(source: &str, line_limit: u32) -> String {
    fmt_options(
        source,
        FormatOptions {
            line_limit,
            ..Default::default()
        },
    )
}
