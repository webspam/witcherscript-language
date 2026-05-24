use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use tree_sitter::{Parser, Tree};

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
    let source = source.into();
    let tree = parser.parse(&source, None).ok_or(ParseError::NoTree)?;
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
mod tests;
