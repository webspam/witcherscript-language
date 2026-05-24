use super::LineIndex;

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
