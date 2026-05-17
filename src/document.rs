use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use tree_sitter::{Parser, Tree};

use crate::diagnostics::{collect_diagnostics, ParseDiagnostic};
use crate::line_index::{LineIndex, SourceRange};
use crate::scope_index::DocScopeIndex;
use crate::symbols::{extract_symbols, DocumentSymbols};

static PARSE_VERSION: AtomicU64 = AtomicU64::new(0);

fn next_parse_version() -> u64 {
    PARSE_VERSION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug)]
pub struct ParsedDocument {
    pub source: String,
    pub tree: Tree,
    pub line_index: LineIndex,
    pub diagnostics: Vec<ParseDiagnostic>,
    pub symbols: DocumentSymbols,
    pub scope_index: DocScopeIndex,
    pub parse_version: u64,
}

#[derive(Debug)]
pub enum ParseError {
    Language(tree_sitter::LanguageError),
    NoTree,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Language(error) => {
                write!(formatter, "failed to load WitcherScript grammar: {error}")
            }
            Self::NoTree => formatter.write_str("tree-sitter returned no parse tree"),
        }
    }
}

impl Error for ParseError {}

pub fn parse_document(source: impl Into<String>) -> Result<ParsedDocument, ParseError> {
    let source = source.into();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_witcherscript::language())
        .map_err(ParseError::Language)?;
    parse_document_with_parser(&mut parser, source)
}

pub fn parse_document_with_parser(
    parser: &mut Parser,
    source: impl Into<String>,
) -> Result<ParsedDocument, ParseError> {
    let source = source.into();
    let tree = parser.parse(&source, None).ok_or(ParseError::NoTree)?;
    let root = tree.root_node();
    let line_index = LineIndex::new(&source);
    let diagnostics = collect_diagnostics(root, &source);
    let symbols = extract_symbols(root, &source, &line_index);
    let scope_index = DocScopeIndex::build(&symbols);

    Ok(ParsedDocument {
        source,
        tree,
        line_index,
        diagnostics,
        symbols,
        scope_index,
        parse_version: next_parse_version(),
    })
}

pub fn apply_content_change(
    source: &str,
    line_index: &LineIndex,
    range: Option<SourceRange>,
    new_text: &str,
) -> Option<String> {
    let Some(range) = range else {
        return Some(new_text.to_owned());
    };

    let start = line_index.position_to_byte(source, range.start)?;
    let end = line_index.position_to_byte(source, range.end)?;
    if start > end {
        return None;
    }

    let mut result = String::with_capacity(source.len() - (end - start) + new_text.len());
    result.push_str(&source[..start]);
    result.push_str(new_text);
    result.push_str(&source[end..]);
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::{apply_content_change, LineIndex};
    use crate::line_index::{SourcePosition, SourceRange};

    fn pos(line: u32, character: u32) -> SourcePosition {
        SourcePosition { line, character }
    }

    fn range(start: SourcePosition, end: SourcePosition) -> Option<SourceRange> {
        Some(SourceRange { start, end })
    }

    fn apply(source: &str, r: Option<SourceRange>, new_text: &str) -> String {
        let index = LineIndex::new(source);
        apply_content_change(source, &index, r, new_text).expect("apply succeeds")
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
        let step1 =
            apply_content_change(source, &index1, range(pos(0, 1), pos(0, 2)), "BB").unwrap();
        assert_eq!(step1, "aBBc\nxyz\n");

        let index2 = LineIndex::new(&step1);
        let step2 =
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
}
