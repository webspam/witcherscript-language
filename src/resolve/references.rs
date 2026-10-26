use tree_sitter::Node;

use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::line_index::{SourcePosition, SourceRange};
use crate::symbols::{AccessLevel, SymbolKind};

use super::Definition;
use super::ast::identifier_at;
use super::definition::{all_declarations_of, definition_key, resolve_definition};
use super::inference::{resolve_local_or_parameter, resolve_name};
use super::symbol_db::SymbolDb;

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
    let type_name = definition.symbol.type_name();
    let base_def = db.base.find_top_level(type_name)?;
    if !suppressed.contains(base_def.uri.as_str()) {
        return None;
    }
    let ws_def = db.workspace.find_top_level(type_name)?;
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
    byte_range: &std::ops::Range<usize>,
    name: &str,
    equiv_keys: &[OccurrenceKey],
    paired_base_top_key: Option<&OccurrenceKey>,
) -> bool {
    if paired_base_top_key
        .is_some_and(|(key_uri, key_range)| key_uri == uri && key_range == byte_range)
    {
        return true;
    }
    let byte_offset = byte_range.start;
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
    equiv_keys.contains(&definition_key(&resolved))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub uri: String,
    pub range: SourceRange,
    pub name_range: SourceRange,
}

impl Reference {
    fn in_document(
        uri: &str,
        document: &ParsedDocument,
        byte_range: &std::ops::Range<usize>,
        name_offset: usize,
    ) -> Self {
        let to_range = |start| {
            document
                .line_index
                .byte_range_to_range(&document.source, start, byte_range.end)
        };
        Self {
            uri: uri.to_string(),
            range: to_range(byte_range.start),
            name_range: to_range(byte_range.start + name_offset),
        }
    }
}

pub fn state_class_name_range_at(
    definition: &Definition,
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<SourceRange> {
    let type_name = definition.symbol.type_name();
    if type_name == definition.symbol.name.as_str() {
        return None;
    }
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let ident = identifier_at(document.tree.root_node(), byte_offset)?;
    let spells_class_name = ident.utf8_text(document.source.as_bytes()).ok()? == type_name;
    spells_class_name.then(|| {
        document.line_index.byte_range_to_range(
            &document.source,
            ident.start_byte(),
            ident.end_byte(),
        )
    })
}

pub fn find_references(
    definition: &Definition,
    definition_document: &ParsedDocument,
    search_documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
    include_declaration: bool,
) -> Vec<Reference> {
    let name = &definition.symbol.name;
    let type_name = definition.symbol.type_name();
    let mut search_names = vec![(name.as_str(), 0)];
    if type_name != name.as_str() {
        search_names.push((type_name, type_name.len() - name.len()));
    }

    // All declarations of the logical member count as one symbol.
    let equiv = all_declarations_of(definition, db);
    let equiv_keys: Vec<OccurrenceKey> = equiv.iter().map(definition_key).collect();

    let scope = if equiv.len() > 1 {
        SearchScope::AllDocuments
    } else {
        definition_search_scope(definition, definition_document)
    };

    let paired_base = paired_suppressed_base_uri(definition, db);
    let paired_base_top_key = paired_base
        .as_ref()
        .and_then(|_| db.base.find_top_level(type_name))
        .map(|d| definition_key(&d));

    let mut results = Vec::new();
    let mut filter = OccurrenceFilter {
        decl_found: vec![false; equiv_keys.len()],
        equiv_keys,
        include_declaration,
    };

    for (uri, document) in search_documents {
        if paired_base.as_deref() == Some(*uri) {
            results.extend(paired_base_references(
                uri,
                document,
                db,
                name,
                paired_base_top_key.as_ref(),
                &mut filter,
            ));
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

        let mut candidates: Vec<(std::ops::Range<usize>, usize)> = search_names
            .iter()
            .flat_map(|&(search_name, name_offset)| {
                collect_candidate_ranges(uri, document, db, search_name, scan_range)
                    .into_iter()
                    .map(move |byte_range| (byte_range, name_offset))
            })
            .collect();
        candidates.sort_by_key(|(byte_range, _)| byte_range.start);

        for (byte_range, name_offset) in candidates {
            // Semantic verification: resolve the candidate and confirm it points
            // at the same definition (same file + same selection range).
            let position = document
                .line_index
                .byte_to_position(&document.source, byte_range.start);
            let resolved = resolve_definition(uri, document, db, position);
            match resolved {
                Some(ref r) if filter.equiv_keys.contains(&definition_key(r)) => {}
                _ => continue,
            }

            if !filter.should_report(&(uri.to_string(), byte_range.clone())) {
                continue;
            }
            results.push(Reference::in_document(
                uri,
                document,
                &byte_range,
                name_offset,
            ));
        }
    }

    // Catch declarations whose file was not in the search set.
    if include_declaration {
        for (idx, decl) in equiv.iter().enumerate() {
            if !filter.decl_found[idx] {
                results.push(Reference {
                    uri: decl.uri.clone(),
                    range: decl.symbol.selection_range,
                    name_range: decl.symbol.selection_range,
                });
            }
        }
    }

    results
}

type OccurrenceKey = (String, std::ops::Range<usize>);

struct OccurrenceFilter {
    equiv_keys: Vec<OccurrenceKey>,
    decl_found: Vec<bool>,
    include_declaration: bool,
}

impl OccurrenceFilter {
    fn should_report(&mut self, occurrence_key: &OccurrenceKey) -> bool {
        match self.equiv_keys.iter().position(|k| k == occurrence_key) {
            Some(idx) => {
                self.decl_found[idx] = true;
                self.include_declaration
            }
            None => true,
        }
    }
}

fn paired_base_references(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    name: &str,
    paired_base_top_key: Option<&OccurrenceKey>,
    filter: &mut OccurrenceFilter,
) -> Vec<Reference> {
    let mut references = Vec::new();
    for byte_range in collect_candidate_ranges(uri, document, db, name, None) {
        if !paired_base_occurrence_matches(
            uri,
            document,
            db,
            &byte_range,
            name,
            &filter.equiv_keys,
            paired_base_top_key,
        ) {
            continue;
        }
        let occurrence_key = (uri.to_string(), byte_range.clone());
        let is_paired_base_declaration = paired_base_top_key == Some(&occurrence_key);
        if !filter.should_report(&occurrence_key)
            || (is_paired_base_declaration && !filter.include_declaration)
        {
            continue;
        }
        references.push(Reference::in_document(uri, document, &byte_range, 0));
    }
    references
}

fn collect_candidate_ranges(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    name: &str,
    scan_range: Option<&std::ops::Range<usize>>,
) -> Vec<std::ops::Range<usize>> {
    let indexed = db.workspace.is_indexed(uri) || db.base.is_indexed(uri);
    if scan_range.is_none() && indexed {
        return db
            .workspace
            .ident_ranges_in_doc(uri, name)
            .or_else(|| db.base.ident_ranges_in_doc(uri, name))
            .map_or_else(Vec::new, <[_]>::to_vec);
    }
    let mut byte_ranges = Vec::new();
    collect_ident_occurrences(
        document.tree.root_node(),
        document.source.as_bytes(),
        name,
        scan_range,
        &mut byte_ranges,
    );
    byte_ranges
}

fn collect_ident_occurrences(
    node: Node<'_>,
    source: &[u8],
    name: &str,
    scope: Option<&std::ops::Range<usize>>,
    results: &mut Vec<std::ops::Range<usize>>,
) {
    if let Some(s) = scope
        && (node.end_byte() <= s.start || node.start_byte() >= s.end)
    {
        return;
    }
    if node.kind() == kinds::IDENT && node.utf8_text(source).ok() == Some(name) {
        results.push(node.start_byte()..node.end_byte());
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ident_occurrences(child, source, name, scope, results);
    }
}
