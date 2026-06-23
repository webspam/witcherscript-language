use std::ops::Range;

#[derive(Debug, Clone)]
pub struct Splice {
    /// Byte range in the original source this edit replaces; an empty range is a pure insertion.
    pub range: Range<usize>,
    pub text: String,
}

/// Whether a refactor's edits are provably free of runtime change.
#[derive(Debug)]
pub enum Confidence {
    Verified,
    Unverified,
}

#[derive(Debug)]
pub struct EditPlan {
    /// Non-overlapping edits against the original source.
    pub edits: Vec<Splice>,
    pub confidence: Confidence,
}

impl EditPlan {
    pub fn apply(&self, source: &str) -> String {
        apply_splices(source, &self.edits)
    }
}

/// An `EditPlan` that also introduces a named symbol, with a caret offset for a follow-up rename.
#[derive(Debug)]
pub struct Extraction {
    pub plan: EditPlan,
    pub name: String,
    /// Byte offset in the applied text where the new name starts, for cursor placement.
    pub cursor: usize,
}

impl Extraction {
    pub(super) fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.plan.confidence = confidence;
        self
    }
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

pub(crate) fn delete_statement(source: &str, stmt: Range<usize>) -> Splice {
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

// Span to the neighbour's boundary, not just the comma, so a multi-line entry collapses instead of orphaning its prefix.
pub(crate) fn remove_list_entry(
    target: &Range<usize>,
    prev: Option<&Range<usize>>,
    next: Option<&Range<usize>>,
) -> Splice {
    let range = match (prev, next) {
        (_, Some(next)) => target.start..next.start,
        (Some(prev), None) => prev.end..target.end,
        (None, None) => target.clone(),
    };
    Splice {
        range,
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
        plan: EditPlan {
            edits,
            confidence: Confidence::Verified,
        },
        name,
        cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remove(
        source: &str,
        target: Range<usize>,
        prev: Option<Range<usize>>,
        next: Option<Range<usize>>,
    ) -> String {
        let splice = remove_list_entry(&target, prev.as_ref(), next.as_ref());
        let mut out = source.to_string();
        out.replace_range(splice.range, &splice.text);
        out
    }

    #[test]
    fn removing_first_entry_takes_the_following_comma() {
        assert_eq!(remove("a, b", 0..1, None, Some(3..4)), "b");
    }

    #[test]
    fn removing_last_entry_takes_the_preceding_comma() {
        assert_eq!(remove("a, b", 3..4, Some(0..1), None), "a");
    }

    #[test]
    fn removing_a_middle_entry_leaves_a_well_formed_list() {
        assert_eq!(remove("a, b, c", 3..4, Some(0..1), Some(6..7)), "a, c");
    }

    #[test]
    fn removing_a_sole_entry_touches_only_itself() {
        assert_eq!(remove("a", 0..1, None, None), "");
    }

    #[test]
    fn removing_a_multiline_entry_collapses_instead_of_orphaning() {
        assert_eq!(remove("a,\n    b", 0..1, None, Some(7..8)), "b");
    }
}
