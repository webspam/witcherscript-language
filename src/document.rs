use std::error::Error;
use std::fmt;

use tree_sitter::{Parser, Tree};

use crate::diagnostics::{collect_diagnostics, ParseDiagnostic};
use crate::line_index::LineIndex;
use crate::symbols::{extract_symbols, DocumentSymbols};

#[derive(Debug)]
pub struct ParsedDocument {
    pub source: String,
    pub tree: Tree,
    pub line_index: LineIndex,
    pub diagnostics: Vec<ParseDiagnostic>,
    pub symbols: DocumentSymbols,
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
    })
}
