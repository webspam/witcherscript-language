use std::collections::HashMap;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::symbols::{Symbol, SymbolId, SymbolKind};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObservedKey {
    TopLevel(String),
    Member(String, String),
    EnumMember(String),
}

pub(super) fn outward_hash_map(symbols: &[Symbol]) -> HashMap<ObservedKey, u64> {
    let mut out: HashMap<ObservedKey, u64> = HashMap::new();
    for s in symbols.iter().filter(|s| is_outward_visible(s)) {
        let key = outward_key_for(s);
        let hash = outward_symbol_hash(s);
        out.entry(key).and_modify(|h| *h ^= hash).or_insert(hash);
    }
    // Parameters fold into their callable's entry; dependents must see parameter-list changes.
    let mut ordinals: HashMap<SymbolId, u64> = HashMap::new();
    for s in symbols.iter().filter(|s| s.kind == SymbolKind::Parameter) {
        let Some(callable) = s.container.and_then(|id| symbols.get(id.0)) else {
            continue;
        };
        let ordinal = ordinals.entry(callable.id).or_insert(0);
        let hash = outward_parameter_hash(s, *ordinal);
        *ordinal += 1;
        let key = outward_key_for(callable);
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
        (false, SymbolKind::EnumMember) => ObservedKey::EnumMember(s.name.clone()),
        (false, _) => {
            ObservedKey::Member(s.container_name.clone().unwrap_or_default(), s.name.clone())
        }
    }
}

fn hash_symbol_fields<H: std::hash::Hasher>(s: &Symbol, h: &mut H) {
    use std::hash::Hash;
    s.name.hash(h);
    (s.kind as u8).hash(h);
    s.container_name.hash(h);
    s.type_annotation.hash(h);
    s.declaration_text.hash(h);
    s.base_class.hash(h);
    s.owner_class.hash(h);
    s.flavour.hash(h);
    h.write_usize(s.annotations.len());
    for a in &s.annotations {
        a.name.hash(h);
        a.argument.hash(h);
    }
    (s.access as u8).hash(h);
    s.is_optional.hash(h);
    s.is_out.hash(h);
}

// DefaultHasher is non-deterministic across builds; used here only within a single process run.
fn outward_symbol_hash(s: &Symbol) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut h = DefaultHasher::new();
    hash_symbol_fields(s, &mut h);
    h.finish()
}

// The ordinal keeps the XOR fold sensitive to parameter reordering.
fn outward_parameter_hash(s: &Symbol, ordinal: u64) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut h = DefaultHasher::new();
    h.write_u64(ordinal);
    hash_symbol_fields(s, &mut h);
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

// DefaultHasher is non-deterministic across builds; used here only within a single process run.
pub(super) fn doc_surface_hash(uri: &str, symbols: &[Symbol]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut h = DefaultHasher::new();
    uri.hash(&mut h);
    // Parameters count as surface: callers' diagnostics depend on them.
    let externally_visible = symbols
        .iter()
        .filter(|s| !matches!(s.kind, SymbolKind::Variable));
    h.write_usize(externally_visible.clone().count());
    for s in externally_visible {
        hash_symbol_fields(s, &mut h);
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

fn collect_all_idents(
    node: Node<'_>,
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
