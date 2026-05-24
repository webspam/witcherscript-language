use super::*;

#[test]
fn parses_single_file_with_cursor_and_span() {
    let f = Fixture::parse(
        "function Foo() {}\n\
         //       ^^^ name\n\
         function Bar() { Fo$0o(); }\n",
    );
    assert_eq!(f.files.len(), 1);
    assert_eq!(f.files[0].uri.as_str(), "file:///main.ws");
    assert_eq!(
        f.files[0].text,
        "function Foo() {}\nfunction Bar() { Foo(); }\n"
    );
    let (uri, pos) = f.cursor.clone().unwrap();
    assert_eq!(uri.as_str(), "file:///main.ws");
    assert_eq!(
        pos,
        Position {
            line: 1,
            character: 19
        }
    );
    let (span_uri, span) = f.span("name");
    assert_eq!(span_uri.as_str(), "file:///main.ws");
    assert_eq!(
        span,
        Range {
            start: Position {
                line: 0,
                character: 9
            },
            end: Position {
                line: 0,
                character: 12
            },
        }
    );
}

#[test]
fn comment_lines_with_stray_caret_are_not_treated_as_annotations() {
    let f = Fixture::parse(
        "function Foo() {}\n\
         // pointer ^ in a regular comment\n\
         function Bar() {}\n",
    );
    assert!(
        f.spans.is_empty(),
        "stray ^ inside a regular // comment must not register a span"
    );
    assert_eq!(
        f.files[0].text,
        "function Foo() {}\n// pointer ^ in a regular comment\nfunction Bar() {}\n",
        "non-annotation comment lines must reach the server as source content"
    );
}

#[test]
fn positions_are_utf16_code_units_not_chars_or_bytes() {
    // 𐀀 (U+10000) is 1 char / 4 UTF-8 bytes / 2 UTF-16 units, so byte/char/UTF-16 disagree.
    let f = Fixture::parse(concat!("abc𐀀def$0\n", "//  ^^^ id\n"));
    let (_, pos) = f.cursor.clone().unwrap();
    assert_eq!(
        pos,
        Position {
            line: 0,
            character: 8
        },
        "cursor after 'abc𐀀def' must be at UTF-16 col 8 (chars=7, bytes=10)"
    );
    let (_, span) = f.span("id");
    assert_eq!(
        span,
        Range {
            start: Position {
                line: 0,
                character: 5
            },
            end: Position {
                line: 0,
                character: 8
            },
        },
        "span carets at annotation chars 4..7 align with content chars d,e,f -> UTF-16 [5, 8)"
    );
}

#[test]
fn parses_multi_file_fixture() {
    let f = Fixture::parse(
        "//- /lib.ws\n\
         function A() {}\n\
         //- /other.ws\n\
         function B() {}\n",
    );
    assert_eq!(f.files.len(), 2);
    assert_eq!(f.files[0].uri.as_str(), "file:///lib.ws");
    assert_eq!(f.files[0].text, "function A() {}\n");
    assert_eq!(f.files[1].uri.as_str(), "file:///other.ws");
    assert_eq!(f.files[1].text, "function B() {}\n");
}
