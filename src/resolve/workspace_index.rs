use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

use super::ast::is_type_like;
use super::completion_catalog::{
    build_callables, build_enum_members, build_types, global_catalog_changed, CompletionCatalog,
};
use super::{annotation_target_class, Definition, ObservationSet};

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    documents: HashMap<String, Vec<Symbol>>,
    top_level_by_name: HashMap<String, Definition>,
    enum_member_by_name: HashMap<String, Definition>,
    superclass_by_name: HashMap<String, String>,
    member_by_type: HashMap<String, HashMap<String, Definition>>,
    annotated_members_by_type: HashMap<String, HashMap<String, Vec<Definition>>>,
    doc_idents: HashMap<String, HashMap<String, Vec<std::ops::Range<usize>>>>,
    doc_surface_hashes: HashMap<String, u64>,
    surface_hash: u64,
    generation: u64,
    doc_outward_hashes: HashMap<String, HashMap<ObservedKey, u64>>,
    top_level_subscribers: HashMap<String, HashSet<String>>,
    member_subscribers: HashMap<(String, String), HashSet<String>>,
    enum_member_subscribers: HashMap<String, HashSet<String>>,
    subscriber_keys: HashMap<String, ObservationSet>,
    completion_catalog: CompletionCatalog,
    completion_catalog_dirty: bool,
    catalog_rebuild_suppressed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObservedKey {
    TopLevel(String),
    Member(String, String),
    EnumMember(String),
}

impl WorkspaceIndex {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn surface_hash(&self) -> u64 {
        self.surface_hash
    }

    pub fn update_document(
        &mut self,
        uri: impl Into<String>,
        document: &ParsedDocument,
    ) -> HashSet<String> {
        let uri: String = uri.into();
        self.remove_from_indices(&uri);
        self.doc_idents.remove(&uri);
        let all_symbols = document.symbols.all().to_vec();
        self.insert_into_indices(&uri, &all_symbols);
        self.doc_idents
            .insert(uri.clone(), scan_ident_occurrences(document));

        let new_hash = doc_surface_hash(&uri, &all_symbols);
        if let Some(old_hash) = self.doc_surface_hashes.insert(uri.clone(), new_hash) {
            self.surface_hash ^= old_hash;
        }
        self.surface_hash ^= new_hash;

        let new_outward = outward_hash_map(&all_symbols);
        let changed_keys = diff_outward_keys(self.doc_outward_hashes.get(&uri), &new_outward);
        self.doc_outward_hashes.insert(uri.clone(), new_outward);
        let invalidated = self.subscribers_of(&changed_keys);

        self.documents.insert(uri, all_symbols);
        self.generation = self.generation.wrapping_add(1);
        if global_catalog_changed(&changed_keys) {
            self.completion_catalog_dirty = true;
            self.maybe_rebuild_completion_catalog();
        }
        invalidated
    }

    pub fn remove_document(&mut self, uri: &str) -> HashSet<String> {
        self.remove_from_indices(uri);
        self.doc_idents.remove(uri);
        self.documents.remove(uri);
        if let Some(old_hash) = self.doc_surface_hashes.remove(uri) {
            self.surface_hash ^= old_hash;
        }
        let changed_keys: Vec<ObservedKey> = self
            .doc_outward_hashes
            .remove(uri)
            .map(|map| map.into_keys().collect())
            .unwrap_or_default();
        let invalidated = self.subscribers_of(&changed_keys);
        self.generation = self.generation.wrapping_add(1);
        if global_catalog_changed(&changed_keys) {
            self.completion_catalog_dirty = true;
            self.maybe_rebuild_completion_catalog();
        }
        invalidated
    }

    pub fn begin_bulk_catalog_update(&mut self) {
        self.catalog_rebuild_suppressed = self.catalog_rebuild_suppressed.saturating_add(1);
    }

    pub fn end_bulk_catalog_update(&mut self) {
        self.catalog_rebuild_suppressed = self.catalog_rebuild_suppressed.saturating_sub(1);
        self.maybe_rebuild_completion_catalog();
    }

    pub fn rebuild_completion_catalog(&mut self) {
        self.completion_catalog = CompletionCatalog {
            callables: Arc::from(build_callables(&self.top_level_by_name)),
            types: Arc::from(build_types(&self.top_level_by_name)),
            enum_members: Arc::from(build_enum_members(&self.enum_member_by_name)),
        };
        self.completion_catalog_dirty = false;
    }

    fn maybe_rebuild_completion_catalog(&mut self) {
        if self.completion_catalog_dirty && self.catalog_rebuild_suppressed == 0 {
            self.rebuild_completion_catalog();
        }
    }

    pub fn callables_catalog(&self) -> Arc<[Definition]> {
        self.completion_catalog.callables.clone()
    }

    pub fn types_catalog(&self) -> Arc<[Definition]> {
        self.completion_catalog.types.clone()
    }

    pub fn enum_members_catalog(&self) -> Arc<[Definition]> {
        self.completion_catalog.enum_members.clone()
    }

    pub fn register_subscription(&mut self, subscriber_uri: &str, observations: ObservationSet) {
        self.unregister_subscription(subscriber_uri);
        for name in &observations.top_level {
            self.top_level_subscribers
                .entry(name.clone())
                .or_default()
                .insert(subscriber_uri.to_string());
        }
        for key in &observations.members {
            self.member_subscribers
                .entry(key.clone())
                .or_default()
                .insert(subscriber_uri.to_string());
        }
        for name in &observations.enum_members {
            self.enum_member_subscribers
                .entry(name.clone())
                .or_default()
                .insert(subscriber_uri.to_string());
        }
        self.subscriber_keys
            .insert(subscriber_uri.to_string(), observations);
    }

    pub fn unregister_subscription(&mut self, subscriber_uri: &str) {
        let Some(prev) = self.subscriber_keys.remove(subscriber_uri) else {
            return;
        };
        for name in prev.top_level {
            if let Some(set) = self.top_level_subscribers.get_mut(&name) {
                set.remove(subscriber_uri);
                if set.is_empty() {
                    self.top_level_subscribers.remove(&name);
                }
            }
        }
        for key in prev.members {
            if let Some(set) = self.member_subscribers.get_mut(&key) {
                set.remove(subscriber_uri);
                if set.is_empty() {
                    self.member_subscribers.remove(&key);
                }
            }
        }
        for name in prev.enum_members {
            if let Some(set) = self.enum_member_subscribers.get_mut(&name) {
                set.remove(subscriber_uri);
                if set.is_empty() {
                    self.enum_member_subscribers.remove(&name);
                }
            }
        }
    }

    fn subscribers_of(&self, keys: &[ObservedKey]) -> HashSet<String> {
        let mut out = HashSet::new();
        for key in keys {
            let bucket = match key {
                ObservedKey::TopLevel(n) => self.top_level_subscribers.get(n),
                ObservedKey::Member(c, n) => self.member_subscribers.get(&(c.clone(), n.clone())),
                ObservedKey::EnumMember(n) => self.enum_member_subscribers.get(n),
            };
            if let Some(set) = bucket {
                for uri in set {
                    out.insert(uri.clone());
                }
            }
        }
        out
    }

    pub(super) fn is_indexed(&self, uri: &str) -> bool {
        self.doc_idents.contains_key(uri)
    }

    /// Approximate heap bytes consumed by the ident occurrence index.
    pub fn doc_idents_bytes(&self) -> usize {
        let mut total = 0usize;
        for (uri, name_map) in &self.doc_idents {
            total += uri.capacity();
            for (name, ranges) in name_map {
                total += name.capacity();
                total += ranges.capacity() * size_of::<std::ops::Range<usize>>();
            }
            // HashMap slot overhead: ~56 bytes per entry (key ptr + value ptr + hash)
            total += name_map.capacity() * 56;
        }
        total += self.doc_idents.capacity() * 56;
        total
    }

    pub(super) fn ident_ranges_in_doc(
        &self,
        uri: &str,
        name: &str,
    ) -> Option<&[std::ops::Range<usize>]> {
        self.doc_idents.get(uri)?.get(name).map(Vec::as_slice)
    }

    fn remove_from_indices(&mut self, uri: &str) {
        let Some(old_symbols) = self.documents.get(uri) else {
            return;
        };
        for sym in old_symbols.clone() {
            if sym.container.is_none() {
                if self
                    .top_level_by_name
                    .get(&sym.name)
                    .map(|d| d.uri == uri)
                    .unwrap_or(false)
                {
                    self.top_level_by_name.remove(&sym.name);
                    if let Some(def) = self
                        .find_replacement_def(uri, |s| s.container.is_none() && s.name == sym.name)
                    {
                        self.top_level_by_name.insert(sym.name.clone(), def);
                    }
                }
                if is_type_like(sym.kind) {
                    self.superclass_by_name.remove(&sym.name);
                    let base = self
                        .find_replacement_def(uri, |s| {
                            s.container.is_none() && is_type_like(s.kind) && s.name == sym.name
                        })
                        .and_then(|def| def.symbol.base_class);
                    if let Some(base) = base {
                        self.superclass_by_name.insert(sym.name.clone(), base);
                    }
                }
                if matches!(sym.kind, SymbolKind::Function | SymbolKind::Field) {
                    if let Some(target) = annotation_target_class(&sym) {
                        if let Some(by_name) = self.annotated_members_by_type.get_mut(target) {
                            if let Some(defs) = by_name.get_mut(&sym.name) {
                                defs.retain(|d| d.uri != uri);
                                if defs.is_empty() {
                                    by_name.remove(&sym.name);
                                }
                            }
                            if by_name.is_empty() {
                                self.annotated_members_by_type.remove(target);
                            }
                        }
                    }
                }
            } else if let Some(cn) = &sym.container_name {
                let owns_member = self
                    .member_by_type
                    .get(cn)
                    .and_then(|m| m.get(&sym.name))
                    .map(|d| d.uri == uri)
                    .unwrap_or(false);
                if owns_member {
                    let replacement = self.find_replacement_def(uri, |s| {
                        s.container_name.as_deref() == Some(cn.as_str()) && s.name == sym.name
                    });
                    let members = self.member_by_type.entry(cn.clone()).or_default();
                    members.remove(&sym.name);
                    if let Some(def) = replacement {
                        members.insert(sym.name.clone(), def);
                    }
                    if members.is_empty() {
                        self.member_by_type.remove(cn);
                    }
                }
                if sym.kind == SymbolKind::EnumMember
                    && self
                        .enum_member_by_name
                        .get(&sym.name)
                        .map(|d| d.uri == uri)
                        .unwrap_or(false)
                {
                    self.enum_member_by_name.remove(&sym.name);
                    if let Some(def) = self.find_replacement_def(uri, |s| {
                        s.kind == SymbolKind::EnumMember && s.name == sym.name
                    }) {
                        self.enum_member_by_name.insert(sym.name.clone(), def);
                    }
                }
            }
        }
    }

    fn find_replacement_def<F>(&self, exclude_uri: &str, predicate: F) -> Option<Definition>
    where
        F: Fn(&Symbol) -> bool,
    {
        self.documents
            .iter()
            .filter(|(other_uri, _)| other_uri.as_str() != exclude_uri)
            .find_map(|(other_uri, syms)| {
                syms.iter().find(|s| predicate(s)).map(|s| Definition {
                    uri: other_uri.clone(),
                    symbol: s.clone(),
                })
            })
    }

    fn insert_into_indices(&mut self, uri: &str, symbols: &[Symbol]) {
        for sym in symbols {
            if sym.container.is_none() {
                self.top_level_by_name.insert(
                    sym.name.clone(),
                    Definition {
                        uri: uri.to_string(),
                        symbol: sym.clone(),
                    },
                );
                if is_type_like(sym.kind) {
                    if let Some(superclass) = &sym.base_class {
                        self.superclass_by_name
                            .insert(sym.name.clone(), superclass.clone());
                    }
                }
                if matches!(sym.kind, SymbolKind::Function | SymbolKind::Field) {
                    if let Some(target) = annotation_target_class(sym) {
                        self.annotated_members_by_type
                            .entry(target.to_string())
                            .or_default()
                            .entry(sym.name.clone())
                            .or_default()
                            .push(Definition {
                                uri: uri.to_string(),
                                symbol: sym.clone(),
                            });
                    }
                }
            } else if let Some(cn) = &sym.container_name {
                self.member_by_type.entry(cn.clone()).or_default().insert(
                    sym.name.clone(),
                    Definition {
                        uri: uri.to_string(),
                        symbol: sym.clone(),
                    },
                );
                if sym.kind == SymbolKind::EnumMember {
                    self.enum_member_by_name.insert(
                        sym.name.clone(),
                        Definition {
                            uri: uri.to_string(),
                            symbol: sym.clone(),
                        },
                    );
                }
            }
        }
    }

    pub fn find_top_level(&self, name: &str) -> Option<Definition> {
        self.top_level_by_name.get(name).cloned()
    }

    pub fn find_enum_member(&self, name: &str) -> Option<Definition> {
        self.enum_member_by_name.get(name).cloned()
    }

    pub fn all_enum_members(&self) -> Vec<Definition> {
        self.enum_members_catalog().iter().cloned().collect()
    }

    pub fn all_types(&self) -> Vec<Definition> {
        self.types_catalog().iter().cloned().collect()
    }

    pub fn all_top_level_callables(&self) -> Vec<Definition> {
        self.callables_catalog().iter().cloned().collect()
    }

    /// Unlike `top_level_by_name`, does not dedup by name — name collisions stay visible.
    pub fn all_top_level(&self) -> impl Iterator<Item = (&str, &Symbol)> {
        self.documents.iter().flat_map(|(uri, symbols)| {
            symbols
                .iter()
                .filter(|sym| sym.container.is_none())
                .map(move |sym| (uri.as_str(), sym))
        })
    }

    pub fn documents(&self) -> impl Iterator<Item = (&str, &[Symbol])> {
        self.documents
            .iter()
            .map(|(uri, syms)| (uri.as_str(), syms.as_slice()))
    }

    pub fn direct_member_of(
        &self,
        container_name: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        self.member_by_type
            .get(container_name)
            .and_then(|members| members.get(name))
            .or_else(|| {
                self.annotated_members_by_type
                    .get(container_name)
                    .and_then(|members| members.get(name))
                    .and_then(|defs| defs.first())
            })
            .filter(|def| def.symbol.access >= min_access)
            .cloned()
    }

    pub fn direct_members_of(
        &self,
        container_name: &str,
        min_access: AccessLevel,
    ) -> Vec<Definition> {
        let class_body = self
            .member_by_type
            .get(container_name)
            .into_iter()
            .flat_map(|m| m.values().cloned());
        let annotated = self
            .annotated_members_by_type
            .get(container_name)
            .into_iter()
            .flat_map(|m| m.values().flatten().cloned());
        class_body
            .chain(annotated)
            .filter(|d| d.symbol.access >= min_access)
            .collect()
    }

    pub(super) fn annotated_members(&self, container_name: &str, name: &str) -> Vec<Definition> {
        self.annotated_members_by_type
            .get(container_name)
            .and_then(|m| m.get(name))
            .cloned()
            .unwrap_or_default()
    }

    pub fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.superclass_by_name.get(class_name).cloned()
    }

    pub fn parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<String> {
        self.full_parameters_of(uri, callable_id)
            .into_iter()
            .filter(|s| !s.is_optional)
            .map(|s| s.name)
            .collect()
    }

    pub fn full_parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<Symbol> {
        let Some(symbols) = self.documents.get(uri) else {
            return vec![];
        };
        symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Parameter && s.container == Some(callable_id))
            .cloned()
            .collect()
    }
}

fn outward_hash_map(symbols: &[Symbol]) -> HashMap<ObservedKey, u64> {
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

fn diff_outward_keys(
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

fn doc_surface_hash(uri: &str, symbols: &[Symbol]) -> u64 {
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

fn scan_ident_occurrences(
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
