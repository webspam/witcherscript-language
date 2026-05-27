use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::diagnostics::{collect_diagnostics, ParseDiagnostic};
use crate::line_index::{LineIndex, SourceRange};
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
    parse_document_with_prior(parser, source, None)
}

// Tree-sitter reuses subtrees that survived `tree.edit()`; small edit -> fast re-parse.
pub fn parse_document_with_prior(
    parser: &mut Parser,
    source: impl Into<String>,
    prior_tree: Option<&Tree>,
) -> Result<ParsedDocument, ParseError> {
    let source = source.into();
    let tree = parser
        .parse(&source, prior_tree)
        .ok_or(ParseError::NoTree)?;
    let root = tree.root_node();
    let line_index = LineIndex::new(&source);
    let diagnostics = collect_diagnostics(root, &source);
    let symbols = extract_symbols(root, &source, &line_index);

    Ok(ParsedDocument {
        source,
        tree,
        line_index,
        diagnostics,
        symbols,
        parse_version: next_parse_version(),
    })
}

pub fn apply_content_change(
    source: &str,
    line_index: &LineIndex,
    range: Option<SourceRange>,
    new_text: &str,
) -> Option<(String, Option<InputEdit>)> {
    let Some(range) = range else {
        return Some((new_text.to_owned(), None));
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

    let new_end = start + new_text.len();
    let edit = InputEdit {
        start_byte: start,
        old_end_byte: end,
        new_end_byte: new_end,
        start_position: byte_point(line_index, start),
        old_end_position: byte_point(line_index, end),
        new_end_position: byte_point_in(&result, new_end),
    };
    Some((result, Some(edit)))
}

fn byte_point(line_index: &LineIndex, byte: usize) -> Point {
    let row = line_index
        .line_starts()
        .partition_point(|line_start| *line_start <= byte)
        .saturating_sub(1);
    let column = byte - line_index.line_starts()[row];
    Point { row, column }
}

fn byte_point_in(source: &str, byte: usize) -> Point {
    let prefix = &source[..byte];
    let row = prefix.bytes().filter(|b| *b == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
    Point {
        row,
        column: byte - line_start,
    }
}

#[cfg(test)]
mod tests;
