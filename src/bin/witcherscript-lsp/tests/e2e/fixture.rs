use std::collections::HashMap;

use lsp_types::{Position, Range, Url};

pub(crate) struct Fixture {
    pub(crate) files: Vec<FixtureFile>,
    pub(crate) cursor: Option<(Url, Position)>,
    pub(crate) spans: HashMap<String, (Url, Range)>,
}

pub(crate) struct FixtureFile {
    pub(crate) uri: Url,
    pub(crate) text: String,
}

impl Fixture {
    pub(crate) fn parse(input: &str) -> Self {
        let mut files: Vec<FixtureFile> = Vec::new();
        let mut cursor: Option<(Url, Position)> = None;
        let mut spans: HashMap<String, (Url, Range)> = HashMap::new();

        let mut current_uri: Url = default_uri();
        let mut current_text = String::new();
        let mut stripped_line_idx: u32 = 0;
        let mut last_content_line_idx: Option<u32> = None;
        let mut prev_content_line: Option<String> = None;
        let mut started = false;

        for raw_line in input.lines() {
            let trimmed = raw_line.trim_start();

            if let Some(path) = trimmed.strip_prefix("//- ") {
                if started {
                    files.push(FixtureFile {
                        uri: current_uri.clone(),
                        text: std::mem::take(&mut current_text),
                    });
                }
                let path = path.trim();
                let path = path.strip_prefix('/').unwrap_or(path);
                current_uri = Url::parse(&format!("file:///{path}"))
                    .expect("fixture: invalid file path after //-");
                stripped_line_idx = 0;
                last_content_line_idx = None;
                prev_content_line = None;
                started = true;
                continue;
            }

            started = true;

            if let Some(after_slashes) = trimmed.strip_prefix("//") {
                if after_slashes.trim_start().starts_with('^') {
                    let mut byte_idx: usize = 0;
                    let mut caret_start_char: Option<usize> = None;
                    let mut caret_end_char: usize = 0;
                    let mut caret_end_byte: usize = 0;
                    for (char_idx, c) in raw_line.chars().enumerate() {
                        if c == '^' {
                            if caret_start_char.is_none() {
                                caret_start_char = Some(char_idx);
                            }
                            caret_end_char = char_idx + 1;
                            caret_end_byte = byte_idx + c.len_utf8();
                        } else if caret_start_char.is_some() {
                            break;
                        }
                        byte_idx += c.len_utf8();
                    }
                    let start_char = caret_start_char.expect("caret confirmed above");

                    let prev_line = prev_content_line.as_deref().unwrap_or("");
                    let mut prev_chars = prev_line.chars();
                    let mut col_u16: u32 = 0;
                    let mut start_col_u16: u32 = 0;
                    let mut end_col_u16: u32 = 0;
                    for i in 0..caret_end_char {
                        let c = prev_chars.next().unwrap_or(' ');
                        if i == start_char {
                            start_col_u16 = col_u16;
                        }
                        col_u16 += c.len_utf16() as u32;
                        if i + 1 == caret_end_char {
                            end_col_u16 = col_u16;
                        }
                    }

                    let label = raw_line[caret_end_byte..].trim();
                    if let Some(prev_idx) = last_content_line_idx {
                        let range = Range {
                            start: Position {
                                line: prev_idx,
                                character: start_col_u16,
                            },
                            end: Position {
                                line: prev_idx,
                                character: end_col_u16,
                            },
                        };
                        let prev = spans.insert(label.to_string(), (current_uri.clone(), range));
                        assert!(prev.is_none(), "fixture: duplicate span label {label:?}");
                    }
                    continue;
                }
            }

            let mut out_line = String::new();
            let mut col: u32 = 0;
            let mut chars = raw_line.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '$' && chars.peek() == Some(&'0') {
                    chars.next();
                    assert!(
                        cursor.is_none(),
                        "fixture: multiple $0 cursor markers (only one allowed)"
                    );
                    cursor = Some((
                        current_uri.clone(),
                        Position {
                            line: stripped_line_idx,
                            character: col,
                        },
                    ));
                    continue;
                }
                out_line.push(c);
                col += c.len_utf16() as u32;
            }
            current_text.push_str(&out_line);
            current_text.push('\n');
            prev_content_line = Some(out_line);
            last_content_line_idx = Some(stripped_line_idx);
            stripped_line_idx += 1;
        }

        files.push(FixtureFile {
            uri: current_uri,
            text: current_text,
        });

        Fixture {
            files,
            cursor,
            spans,
        }
    }

    pub(crate) fn cursor(&self) -> (Url, Position) {
        self.cursor
            .clone()
            .expect("fixture has no $0 cursor marker")
    }

    pub(crate) fn span(&self, label: &str) -> (Url, Range) {
        self.spans
            .get(label)
            .cloned()
            .unwrap_or_else(|| panic!("fixture has no span labelled {label:?}"))
    }
}

fn default_uri() -> Url {
    Url::parse("file:///main.ws").unwrap()
}

#[cfg(test)]
mod tests {
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
}
