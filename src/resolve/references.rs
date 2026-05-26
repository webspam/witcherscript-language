use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourceRange;
use crate::symbols::{AccessLevel, SymbolKind};

use super::ast::identifier_at;
use super::definition::{all_declarations_of, definition_key, resolve_definition};
use super::inference::{resolve_local_or_parameter, resolve_name};
use super::symbol_db::SymbolDb;
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

// A legacy override keeps the vanilla file on disk; references there share the effective top-level name.
fn paired_suppressed_base_uri(definition: &Definition, db: &SymbolDb) -> Option<String> {
    if definition.symbol.container.is_some() {
        return None;
    }
    let suppressed = db.suppressed_base_uris()?;
    let base_def = db.base.find_top_level(&definition.symbol.name)?;
    if !suppressed.contains(base_def.uri.as_str()) {
        return None;
    }
    let ws_def = db.workspace.find_top_level(&definition.symbol.name)?;
    (ws_def.uri == definition.uri).then_some(base_def.uri)
}

fn ident_is_binding_declaration(ident: Node<'_>, document: &ParsedDocument) -> bool {
    let Some(name) = ident.utf8_text(document.source.as_bytes()).ok() else {
        return false;
    };
    document.symbols.all().iter().any(|symbol| {
        symbol.kind == SymbolKind::Variable
            && symbol.name == name
            && symbol.selection_byte_range.start == ident.start_byte()
            && symbol.selection_byte_range.end == ident.end_byte()
    })
}

fn paired_base_occurrence_matches(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
    name: &str,
    equiv_keys: &[(String, std::ops::Range<usize>)],
    paired_base_top_key: Option<&(String, std::ops::Range<usize>)>,
) -> bool {
    if resolve_local_or_parameter(uri, document, byte_offset, name).is_some() {
        return false;
    }
    let Some(ident) = identifier_at(document.tree.root_node(), byte_offset) else {
        return false;
    };
    if ident_is_binding_declaration(ident, document) {
        return false;
    }
    let Some(resolved) = resolve_name(uri, document, db, byte_offset, name) else {
        return false;
    };
    let key = definition_key(&resolved);
    if equiv_keys.contains(&key) {
        return true;
    }
    if paired_base_top_key == Some(&key) {
        return ident.start_byte() == resolved.symbol.selection_byte_range.start
            && ident.end_byte() == resolved.symbol.selection_byte_range.end;
    }
    false
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

    let paired_base = paired_suppressed_base_uri(definition, db);
    let paired_base_top_key = paired_base
        .as_ref()
        .and_then(|_| db.base.find_top_level(name))
        .map(|d| definition_key(&d));

    let mut results = Vec::new();
    let mut decl_found = vec![false; equiv.len()];

    for (uri, document) in search_documents {
        if paired_base.as_deref() == Some(*uri) {
            let mut byte_ranges: Vec<std::ops::Range<usize>> = Vec::new();
            collect_ident_occurrences(
                document.tree.root_node(),
                document.source.as_bytes(),
                name,
                None,
                &mut byte_ranges,
            );
            for byte_range in byte_ranges {
                if !paired_base_occurrence_matches(
                    uri,
                    document,
                    db,
                    byte_range.start,
                    name,
                    &equiv_keys,
                    paired_base_top_key.as_ref(),
                ) {
                    continue;
                }
                let range = document.line_index.byte_range_to_range(
                    &document.source,
                    byte_range.start,
                    byte_range.end,
                );
                results.push((uri.to_string(), range));
            }
            continue;
        }

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
