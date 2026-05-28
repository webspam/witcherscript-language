use std::collections::HashMap;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::symbols::{Symbol, SymbolKind};

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
