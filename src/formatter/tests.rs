use crate::document::parse_document;

mod alignment;
mod basic;
mod blank_lines;
mod calls;
mod colon_spacing;
mod comments;
mod line_breaking;
mod structures;

use super::{AnnotationPlacement, FormatOptions};

fn default_options() -> FormatOptions {
    FormatOptions {
        tab_size: 4,
        use_tabs: false,
        line_limit: 100,
        compact_colon: false,
        align_member_colons: false,
        annotation_placement: AnnotationPlacement::Preserve,
    }
}

fn fmt_options(source: &str, options: FormatOptions) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(doc.tree.root_node(), &doc.source, options)
}

pub(super) fn fmt(source: &str) -> String {
    fmt_options(source, default_options())
}

pub(super) fn fmt_compact_colon(source: &str) -> String {
    fmt_options(
        source,
        FormatOptions {
            compact_colon: true,
            ..default_options()
        },
    )
}

pub(super) fn fmt_aligned(source: &str) -> String {
    fmt_options(
        source,
        FormatOptions {
            align_member_colons: true,
            ..default_options()
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
            ..default_options()
        },
    )
}

pub(super) fn fmt_limit(source: &str, line_limit: u32) -> String {
    fmt_options(
        source,
        FormatOptions {
            line_limit,
            ..default_options()
        },
    )
}
