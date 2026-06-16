use std::ops::Range;

#[derive(Debug, Clone)]
pub struct Splice {
    /// Byte range in the original source this edit replaces; an empty range is a pure insertion.
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Debug)]
pub struct Extraction {
    /// Non-overlapping edits against the original source.
    pub edits: Vec<Splice>,
    pub name: String,
    /// Byte offset in the applied text where the new name starts, for cursor placement.
    pub cursor: usize,
}

impl Extraction {
    pub fn apply(&self, source: &str) -> String {
        apply_splices(source, &self.edits)
    }
}

/// Whether a refactor's edits are provably free of runtime change.
pub enum Confidence {
    Verified,
    Unverified,
}

pub struct EditPlan {
    pub edits: Vec<Splice>,
    pub confidence: Confidence,
}

// Splice rightmost-first so each replace_range leaves earlier byte offsets untouched.
pub(super) fn apply_splices(text: &str, splices: &[Splice]) -> String {
    let mut ordered: Vec<&Splice> = splices.iter().collect();
    ordered.sort_by_key(|s| std::cmp::Reverse(s.range.start));
    let mut applied = text.to_string();
    for splice in ordered {
        applied.replace_range(splice.range.clone(), &splice.text);
    }
    applied
}

pub(super) fn delete_statement(source: &str, stmt: Range<usize>) -> Splice {
    let bytes = source.as_bytes();
    let mut start = stmt.start;
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    let at_line_start = start == 0 || bytes[start - 1] == b'\n';

    let mut end = stmt.end;
    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t') {
        end += 1;
    }
    if at_line_start {
        if end < bytes.len() && bytes[end] == b'\r' {
            end += 1;
        }
        if end < bytes.len() && bytes[end] == b'\n' {
            end += 1;
        }
    } else {
        // Other code shares the statement's line, so keep that code and its indentation.
        start = stmt.start;
    }

    Splice {
        range: start..end,
        text: String::new(),
    }
}

// Where `original` lands after the edits apply: shift it past every edit that ends at or before it.
pub(super) fn applied_offset(edits: &[Splice], original: usize) -> usize {
    edits
        .iter()
        .filter(|s| s.range.end <= original)
        .fold(original, |pos, s| pos + s.text.len() - s.range.len())
}

// The cursor lands where the replacement moves to, shifted into it by `cursor_prefix`.
pub(super) fn insert_and_replace(
    insert_at: usize,
    insert_text: String,
    replace: Range<usize>,
    replace_text: String,
    cursor_prefix: usize,
    name: String,
) -> Extraction {
    let cursor_anchor = replace.start;
    let edits = vec![
        Splice {
            range: insert_at..insert_at,
            text: insert_text,
        },
        Splice {
            range: replace,
            text: replace_text,
        },
    ];
    let cursor = applied_offset(&edits, cursor_anchor) + cursor_prefix;
    Extraction {
        edits,
        name,
        cursor,
    }
}
