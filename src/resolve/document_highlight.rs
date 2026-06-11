use tree_sitter::Node;

use crate::cst::grammar::{DefaultOrHintKind, ident_default_or_hint_kind, is_assignment_target};
use crate::document::ParsedDocument;
use crate::line_index::{SourcePosition, SourceRange};

use super::SymbolDb;
use super::ast::identifier_at;
use super::name_context::is_named_binding;
use super::references::find_references;
use super::resolve_definition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    Read,
    Write,
}

/// All occurrences of the symbol under `position`, within `uri` only.
/// `None` when no symbol resolves at the cursor.
pub fn document_highlights(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<(SourceRange, HighlightKind)>> {
    let definition = resolve_definition(uri, document, db, position)?;

    // Single-doc search; find_references still narrows private/local scope internally.
    let refs = find_references(&definition, document, &[(uri, document)], db, true);

    let root = document.tree.root_node();
    let mut highlights: Vec<(SourceRange, HighlightKind)> = refs
        .into_iter()
        // find_references appends declarations from unscanned files; drop those.
        .filter(|(ref_uri, _)| ref_uri == uri)
        .map(|(_, range)| {
            let kind = document
                .line_index
                .position_to_byte(&document.source, range.start)
                .and_then(|byte| identifier_at(root, byte))
                .map_or(HighlightKind::Read, classify_occurrence);
            (range, kind)
        })
        .collect();

    highlights.sort_by_key(|(range, _)| (range.start.line, range.start.character));
    Some(highlights)
}

fn classify_occurrence(ident: Node) -> HighlightKind {
    if is_declaration_name(ident)
        || ident_default_or_hint_kind(ident) == Some(DefaultOrHintKind::Default)
        || is_assignment_target(ident)
    {
        return HighlightKind::Write;
    }
    HighlightKind::Read
}

fn is_declaration_name(ident: Node) -> bool {
    let Some(parent) = ident.parent() else {
        return false;
    };
    is_named_binding(ident, parent)
}
