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
mod tests;
