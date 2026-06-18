use super::*;

#[test]
fn converts_cursor_and_span_to_lsp_types() {
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
    let (uri, pos) = f.cursor();
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
fn converts_each_split_file_uri_to_url() {
    let f = Fixture::parse(
        "//- /lib.ws\n\
         function A() {}\n\
         //- /other.ws\n\
         function B() {}\n",
    );
    assert_eq!(f.files.len(), 2);
    assert_eq!(f.files[0].uri.as_str(), "file:///lib.ws");
    assert_eq!(f.files[1].uri.as_str(), "file:///other.ws");
}
