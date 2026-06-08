use super::{LineIndex, apply_content_change};
use crate::line_index::{SourcePosition, SourceRange};

fn pos(line: u32, character: u32) -> SourcePosition {
    SourcePosition { line, character }
}

fn range(start: SourcePosition, end: SourcePosition) -> Option<SourceRange> {
    Some(SourceRange { start, end })
}

fn apply(source: &str, r: Option<SourceRange>, new_text: &str) -> String {
    let index = LineIndex::new(source);
    apply_content_change(source, &index, r, new_text)
        .expect("apply succeeds")
        .0
}

#[test]
fn inserts_mid_line() {
    let result = apply("hello world", range(pos(0, 5), pos(0, 5)), ",");
    assert_eq!(result, "hello, world");
}

#[test]
fn deletes_range() {
    let result = apply("hello, world", range(pos(0, 5), pos(0, 7)), "");
    assert_eq!(result, "helloworld");
}

#[test]
fn replaces_multiline_range_with_shorter_text() {
    let source = "line one\nline two\nline three\n";
    let result = apply(source, range(pos(0, 5), pos(2, 5)), "X");
    assert_eq!(result, "line Xthree\n");
}

#[test]
fn inserts_at_eof() {
    let source = "abc";
    let result = apply(source, range(pos(0, 3), pos(0, 3)), "def");
    assert_eq!(result, "abcdef");
}

#[test]
fn full_replace_when_range_is_none() {
    let result = apply("anything", None, "fresh contents");
    assert_eq!(result, "fresh contents");
}

#[test]
fn sequence_of_two_changes() {
    let source = "abc\nxyz\n";

    let index1 = LineIndex::new(source);
    let (step1, _) =
        apply_content_change(source, &index1, range(pos(0, 1), pos(0, 2)), "BB").unwrap();
    assert_eq!(step1, "aBBc\nxyz\n");

    let index2 = LineIndex::new(&step1);
    let (step2, _) =
        apply_content_change(&step1, &index2, range(pos(1, 0), pos(1, 3)), "YYY").unwrap();
    assert_eq!(step2, "aBBc\nYYY\n");
}

#[test]
fn handles_utf16_surrogate_pair() {
    let source = "ab\u{10437}cd";
    let result = apply(source, range(pos(0, 4), pos(0, 4)), "Z");
    assert_eq!(result, "ab\u{10437}Zcd");
}

#[test]
fn second_change_position_requires_first_change_applied() {
    let source = "abc";

    let index1 = LineIndex::new(source);
    let (step1, _) =
        apply_content_change(source, &index1, range(pos(0, 3), pos(0, 3)), "def").unwrap();
    assert_eq!(step1, "abcdef");

    let index2 = LineIndex::new(&step1);
    let (step2, _) =
        apply_content_change(&step1, &index2, range(pos(0, 5), pos(0, 6)), "").unwrap();
    assert_eq!(step2, "abcde");

    let index_orig = LineIndex::new(source);
    assert!(
        apply_content_change(source, &index_orig, range(pos(0, 5), pos(0, 6)), "").is_none(),
        "skipping step 1 must produce an out-of-range failure",
    );
}

#[test]
fn out_of_range_position_returns_none() {
    let source = "short";
    let index = LineIndex::new(source);
    let bad = SourceRange {
        start: pos(0, 100),
        end: pos(0, 100),
    };
    assert!(apply_content_change(source, &index, Some(bad), "x").is_none());
}

#[test]
fn preserves_crlf_around_splice() {
    let source = "alpha\r\nbravo\r\ncharlie\r\n";
    let result = apply(source, range(pos(1, 0), pos(1, 5)), "BRAVO");
    assert_eq!(result, "alpha\r\nBRAVO\r\ncharlie\r\n");
}
