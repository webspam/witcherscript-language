use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourceRange;
use crate::symbols::{AccessLevel, SymbolKind};

use super::db::SymbolDb;
use super::definition::{all_declarations_of, definition_key, resolve_definition};
use super::Definition;

enum SearchScope {
    AllDocuments,
    SingleFile,
    SingleFileRange(std::ops::Range<usize>),
}

fn definition_search_scope(
    definition: &Definition,
    definition_document: &ParsedDocument,
) -> SearchScope {
    let container_range = || {
        definition
            .symbol
            .container
            .and_then(|id| definition_document.symbols.by_id(id))
            .map(|container| container.byte_range.clone())
    };

    match definition.symbol.kind {
        SymbolKind::Variable | SymbolKind::Parameter => match container_range() {
            Some(r) => SearchScope::SingleFileRange(r),
            None => SearchScope::SingleFile,
        },
        SymbolKind::Method | SymbolKind::Field
            if definition.symbol.access == AccessLevel::Private =>
        {
            match container_range() {
                Some(r) => SearchScope::SingleFileRange(r),
                None => SearchScope::SingleFile,
            }
        }
        _ => SearchScope::AllDocuments,
    }
}

pub fn find_references(
    definition: &Definition,
    definition_document: &ParsedDocument,
    search_documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
    include_declaration: bool,
) -> Vec<(String, SourceRange)> {
    let name = &definition.symbol.name;

    // All declarations of the logical member count as one symbol.
    let equiv = all_declarations_of(definition, db);
    let equiv_keys: Vec<(String, std::ops::Range<usize>)> =
        equiv.iter().map(definition_key).collect();

    let scope = if equiv.len() > 1 {
        SearchScope::AllDocuments
    } else {
        definition_search_scope(definition, definition_document)
    };

    let mut results = Vec::new();
    let mut decl_found = vec![false; equiv.len()];

    for (uri, document) in search_documents {
        let scan_range: Option<&std::ops::Range<usize>> = match &scope {
            SearchScope::AllDocuments => None,
            SearchScope::SingleFile => {
                if *uri != definition.uri.as_str() {
                    continue;
                }
                None
            }
            SearchScope::SingleFileRange(r) => {
                if *uri != definition.uri.as_str() {
                    continue;
                }
                Some(r)
            }
        };

        let mut byte_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        if scan_range.is_none() {
            if db.workspace.is_indexed(uri) || db.base.is_indexed(uri) {
                // Document is in the index: use it. If the name isn't present,
                // both calls return None and byte_ranges stays empty — no tree scan.
                if let Some(ranges) = db
                    .workspace
                    .ident_ranges_in_doc(uri, name)
                    .or_else(|| db.base.ident_ranges_in_doc(uri, name))
                {
                    byte_ranges.extend_from_slice(ranges);
                }
            } else {
                collect_ident_occurrences(
                    document.tree.root_node(),
                    document.source.as_bytes(),
                    name,
                    None,
                    &mut byte_ranges,
                );
            }
        } else {
            collect_ident_occurrences(
                document.tree.root_node(),
                document.source.as_bytes(),
                name,
                scan_range,
                &mut byte_ranges,
            );
        }

        for byte_range in byte_ranges {
            // Semantic verification: resolve the candidate and confirm it points
            // at the same definition (same file + same selection range).
            let position = document
                .line_index
                .byte_to_position(&document.source, byte_range.start);
            let resolved = resolve_definition(uri, document, db, position);
            match resolved {
                Some(ref r) if equiv_keys.contains(&definition_key(r)) => {}
                _ => continue,
            }

            let occurrence_key = (uri.to_string(), byte_range.clone());
            if let Some(idx) = equiv_keys.iter().position(|k| *k == occurrence_key) {
                decl_found[idx] = true;
                if !include_declaration {
                    continue;
                }
            }
            let range = document.line_index.byte_range_to_range(
                &document.source,
                byte_range.start,
                byte_range.end,
            );
            results.push((uri.to_string(), range));
        }
    }

    // Catch declarations whose file was not in the search set.
    if include_declaration {
        for (idx, decl) in equiv.iter().enumerate() {
            if !decl_found[idx] {
                results.push((decl.uri.clone(), decl.symbol.selection_range));
            }
        }
    }

    results
}

fn collect_ident_occurrences<'tree>(
    node: Node<'tree>,
    source: &[u8],
    name: &str,
    scope: Option<&std::ops::Range<usize>>,
    results: &mut Vec<std::ops::Range<usize>>,
) {
    if let Some(s) = scope {
        if node.end_byte() <= s.start || node.start_byte() >= s.end {
            return;
        }
    }
    if node.kind() == "ident" && node.utf8_text(source).ok() == Some(name) {
        results.push(node.start_byte()..node.end_byte());
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ident_occurrences(child, source, name, scope, results);
    }
}
