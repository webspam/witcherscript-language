use std::collections::HashMap;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourceRange;
use crate::symbols::{AccessLevel, Symbol, SymbolKind};

use super::ast::{find_ancestor_of_kind, nodes_at_offset};
use super::db::{ObservedKey, SymbolDb};
use super::definition::resolve_definition;
use super::inference::{enclosing_type_context, resolve_document_top_level};
use super::{annotation_target_class, dedup_definitions, Definition};

enum SearchScope {
    AllDocuments,
    SingleFile,
    SingleFileRange(std::ops::Range<usize>),
}

/// The `(container, member name)` a symbol logically belongs to, resolving
/// annotation functions to the class they target.
pub(super) fn logical_member(symbol: &Symbol) -> Option<(String, String)> {
    match symbol.kind {
        SymbolKind::Field if symbol.container.is_none() => {
            annotation_target_class(symbol).map(|t| (t.to_string(), symbol.name.clone()))
        }
        SymbolKind::Method | SymbolKind::Field => symbol
            .container_name
            .as_deref()
            .map(|cn| (cn.to_string(), symbol.name.clone())),
        SymbolKind::Function if symbol.container.is_none() => {
            annotation_target_class(symbol).map(|t| (t.to_string(), symbol.name.clone()))
        }
        _ => None,
    }
}

pub(super) fn definition_key(definition: &Definition) -> (String, std::ops::Range<usize>) {
    (
        definition.uri.clone(),
        definition.symbol.selection_byte_range.clone(),
    )
}

/// Every declaration of the same logical member as `definition`, including
/// `definition` itself.
fn member_equivalence_set(definition: &Definition, db: &SymbolDb) -> Vec<Definition> {
    let Some((container, name)) = logical_member(&definition.symbol) else {
        return vec![definition.clone()];
    };
    let mut decls = db.all_member_declarations(&container, &name);
    if !decls
        .iter()
        .any(|d| definition_key(d) == definition_key(definition))
    {
        decls.push(definition.clone());
    }
    dedup_definitions(decls)
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
    let equiv = member_equivalence_set(definition, db);
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

pub(super) fn outward_hash_map(symbols: &[Symbol]) -> HashMap<ObservedKey, u64> {
    let mut out: HashMap<ObservedKey, u64> = HashMap::new();
    for s in symbols.iter().filter(|s| is_outward_visible(s)) {
        let key = outward_key_for(s);
        let hash = outward_symbol_hash(s);
        out.entry(key).and_modify(|h| *h ^= hash).or_insert(hash);
    }
    out
}

fn is_outward_visible(s: &Symbol) -> bool {
    !matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter)
}

fn outward_key_for(s: &Symbol) -> ObservedKey {
    match (s.container.is_none(), s.kind) {
        (true, _) => ObservedKey::TopLevel(s.name.clone()),
        (false, SymbolKind::EnumVariant) => ObservedKey::EnumVariant(s.name.clone()),
        (false, _) => {
            ObservedKey::Member(s.container_name.clone().unwrap_or_default(), s.name.clone())
        }
    }
}

fn outward_symbol_hash(s: &Symbol) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.name.hash(&mut h);
    (s.kind as u8).hash(&mut h);
    s.container_name.hash(&mut h);
    s.type_annotation.hash(&mut h);
    s.signature.hash(&mut h);
    s.base_class.hash(&mut h);
    s.owner_class.hash(&mut h);
    s.flavour.hash(&mut h);
    h.write_usize(s.annotations.len());
    for a in &s.annotations {
        a.name.hash(&mut h);
        a.argument.hash(&mut h);
    }
    (s.access as u8).hash(&mut h);
    s.is_optional.hash(&mut h);
    s.is_out.hash(&mut h);
    h.finish()
}

pub(super) fn diff_outward_keys(
    prev: Option<&HashMap<ObservedKey, u64>>,
    next: &HashMap<ObservedKey, u64>,
) -> Vec<ObservedKey> {
    let mut changed = Vec::new();
    match prev {
        None => {
            for k in next.keys() {
                changed.push(k.clone());
            }
        }
        Some(prev) => {
            for (k, v) in next {
                if prev.get(k) != Some(v) {
                    changed.push(k.clone());
                }
            }
            for k in prev.keys() {
                if !next.contains_key(k) {
                    changed.push(k.clone());
                }
            }
        }
    }
    changed
}

pub(super) fn doc_surface_hash(uri: &str, symbols: &[Symbol]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut h = DefaultHasher::new();
    uri.hash(&mut h);
    let externally_visible = symbols
        .iter()
        .filter(|s| !matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter));
    h.write_usize(externally_visible.clone().count());
    for s in externally_visible {
        s.name.hash(&mut h);
        (s.kind as u8).hash(&mut h);
        s.container_name.hash(&mut h);
        s.type_annotation.hash(&mut h);
        s.signature.hash(&mut h);
        s.base_class.hash(&mut h);
        s.owner_class.hash(&mut h);
        s.flavour.hash(&mut h);
        h.write_usize(s.annotations.len());
        for a in &s.annotations {
            a.name.hash(&mut h);
            a.argument.hash(&mut h);
        }
        (s.access as u8).hash(&mut h);
        s.is_optional.hash(&mut h);
        s.is_out.hash(&mut h);
    }
    h.finish()
}

pub(super) fn scan_ident_occurrences(
    document: &ParsedDocument,
) -> HashMap<String, Vec<std::ops::Range<usize>>> {
    let mut map: HashMap<String, Vec<std::ops::Range<usize>>> = HashMap::new();
    collect_all_idents(
        document.tree.root_node(),
        document.source.as_bytes(),
        &mut map,
    );
    map
}

fn collect_all_idents<'tree>(
    node: Node<'tree>,
    source: &[u8],
    map: &mut HashMap<String, Vec<std::ops::Range<usize>>>,
) {
    if node.kind() == "ident" {
        if let Ok(name) = node.utf8_text(source) {
            map.entry(name.to_string())
                .or_default()
                .push(node.start_byte()..node.end_byte());
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_all_idents(child, source, map);
    }
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

pub(super) fn resolve_self_keyword(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<Definition> {
    let root = document.tree.root_node();
    let node = nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|n| find_ancestor_of_kind(n, &["this_expr", "super_expr", "parent_expr"]))?;

    let current_type = enclosing_type_context(document, db, byte_offset)?;
    match node.kind() {
        "this_expr" => resolve_document_top_level(uri, document, &current_type.name)
            .or_else(|| db.find_top_level(&current_type.name)),
        "super_expr" => {
            let base_name = current_type.base_class.as_deref()?;
            resolve_document_top_level(uri, document, base_name)
                .or_else(|| db.find_top_level(base_name))
        }
        "parent_expr" => {
            let owner_name = current_type.owner_class.as_deref()?;
            resolve_document_top_level(uri, document, owner_name)
                .or_else(|| db.find_top_level(owner_name))
        }
        _ => None,
    }
}

pub(super) fn resolve_at_definition_site(
    uri: &str,
    document: &ParsedDocument,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    document
        .symbols
        .all()
        .iter()
        .find(|symbol| {
            symbol.name == name
                && symbol.selection_byte_range.start <= byte_offset
                && byte_offset < symbol.selection_byte_range.end
        })
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
}
