use rstest::rstest;

use super::{LineIndex, SourcePosition};

#[test]
fn maps_byte_offsets_to_utf16_positions() {
    let source = "alpha\nβeta\n";
    let index = LineIndex::new(source);

    assert_eq!(index.byte_to_position(source, 0).line, 0);
    assert_eq!(index.byte_to_position(source, 6).line, 1);
    assert_eq!(index.byte_to_position(source, 8).character, 1);
}

#[test]
fn maps_utf16_positions_to_byte_offsets() {
    let source = "alpha\nβeta\n";
    let index = LineIndex::new(source);

    assert_eq!(
        index.position_to_byte(
            source,
            super::SourcePosition {
                line: 1,
                character: 1
            }
        ),
        Some(8)
    );
}

// "a😀b": 'a' is byte 0 / unit 0, '😀' is bytes 1..5 / units 1-2, 'b' is byte 5 / unit 3.
#[rstest]
#[case::line_start(0, Some(0))]
#[case::astral_char_start(1, Some(1))]
#[case::mid_surrogate_pair(2, None)]
#[case::after_astral_char(3, Some(5))]
#[case::line_end(4, Some(6))]
#[case::past_line_end(5, None)]
fn position_to_byte_respects_surrogate_pairs(
    #[case] character: u32,
    #[case] expected: Option<usize>,
) {
    let source = "a😀b";
    let index = LineIndex::new(source);
    let pos = SourcePosition { line: 0, character };
    assert_eq!(
        index.position_to_byte(source, pos),
        expected,
        "character {character}"
    );
}

#[rstest]
#[case::line_start(0, 0)]
#[case::astral_char_start(1, 1)]
#[case::after_astral_char(5, 3)]
fn byte_to_position_counts_utf16_units(#[case] byte: usize, #[case] expected_char: u32) {
    let source = "a😀b";
    let index = LineIndex::new(source);
    let pos = index.byte_to_position(source, byte);
    assert_eq!(pos.line, 0, "byte {byte} line");
    assert_eq!(pos.character, expected_char, "byte {byte} character");
}

#[test]
fn position_to_byte_none_when_line_out_of_range() {
    let source = "a\nb\n";
    let index = LineIndex::new(source);
    let pos = SourcePosition {
        line: 9,
        character: 0,
    };
    assert_eq!(
        index.position_to_byte(source, pos),
        None,
        "a line past the last must not resolve to a byte"
    );
}
