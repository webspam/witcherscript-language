use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::script_env::ScriptEnvironment;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

use super::ast::is_type_like;
use super::{
    annotation_target_class, dedup_by_name, dedup_definitions, Definition, ObservationSet,
    MAX_INHERITANCE_DEPTH,
};

const OBJECT_BASE_CLASS: &str = "CObject";

// These sit at/above CObject; a virtual CObject base would form a cycle.
const OBJECT_ROOT_CHAIN: [&str; 3] = ["CObject", "IScriptable", "ISerializable"];

pub struct SymbolDb<'a> {
    pub(super) workspace: &'a WorkspaceIndex,
    pub(super) base: &'a WorkspaceIndex,
    builtins: Option<&'a WorkspaceIndex>,
    script_env: Option<&'a ScriptEnvironment>,
    observer: Option<&'a Mutex<ObservationSet>>,
}

impl<'a> SymbolDb<'a> {
    pub fn new(workspace: &'a WorkspaceIndex, base: &'a WorkspaceIndex) -> Self {
        Self {
            workspace,
            base,
            builtins: None,
            script_env: None,
            observer: None,
        }
    }

    pub fn with_script_env(mut self, env: &'a ScriptEnvironment) -> Self {
        self.script_env = Some(env);
        self
    }

    pub fn with_builtins(mut self, builtins: &'a WorkspaceIndex) -> Self {
        self.builtins = Some(builtins);
        self
    }

    pub fn with_observer<'b>(&self, observer: &'b Mutex<ObservationSet>) -> SymbolDb<'b>
    where
        'a: 'b,
    {
        SymbolDb {
            workspace: self.workspace,
            base: self.base,
            builtins: self.builtins,
            script_env: self.script_env,
            observer: Some(observer),
        }
    }

    pub fn merge_observations(&self, other: ObservationSet) {
        let Some(outer) = self.observer else {
            return;
        };
        let mut outer = outer.lock().expect("observer mutex poisoned");
        outer.top_level.extend(other.top_level);
        outer.members.extend(other.members);
        outer.enum_variants.extend(other.enum_variants);
    }

    fn record_top_level(&self, name: &str) {
        if let Some(obs) = self.observer {
            let mut o = obs.lock().expect("observer mutex poisoned");
            if !o.top_level.contains(name) {
                o.top_level.insert(name.to_string());
            }
        }
    }

    fn record_member(&self, container: &str, name: &str) {
        if let Some(obs) = self.observer {
            let mut o = obs.lock().expect("observer mutex poisoned");
            let key = (container.to_string(), name.to_string());
            if !o.members.contains(&key) {
                o.members.insert(key);
            }
        }
    }

    fn record_enum_variant(&self, name: &str) {
        if let Some(obs) = self.observer {
            let mut o = obs.lock().expect("observer mutex poisoned");
            if !o.enum_variants.contains(name) {
                o.enum_variants.insert(name.to_string());
            }
        }
    }

    pub(super) fn find_script_global(&self, name: &str) -> Option<Definition> {
        let g = self.script_env?.find(name)?;
        if let Some(class_def) = self.find_top_level(&g.type_name) {
            return Some(class_def);
        }
        Some(Definition {
            uri: g.ini_uri.clone(),
            symbol: g.symbol.clone(),
        })
    }

    pub(super) fn script_global_type(&self, name: &str) -> Option<String> {
        self.script_env?.find(name).map(|g| g.type_name.clone())
    }

    pub fn find_top_level(&self, name: &str) -> Option<Definition> {
        self.record_top_level(name);
        self.workspace
            .find_top_level(name)
            .or_else(|| self.base.find_top_level(name))
            .or_else(|| self.builtins.and_then(|b| b.find_top_level(name)))
    }

    pub fn find_enum_variant(&self, name: &str) -> Option<Definition> {
        self.record_enum_variant(name);
        self.workspace
            .find_enum_variant(name)
            .or_else(|| self.base.find_enum_variant(name))
            .or_else(|| self.builtins.and_then(|b| b.find_enum_variant(name)))
    }

    pub fn all_enum_variants(&self) -> Vec<Definition> {
        dedup_by_name(
            self.workspace
                .all_enum_variants()
                .into_iter()
                .chain(self.base.all_enum_variants())
                .chain(
                    self.builtins
                        .map(|b| b.all_enum_variants())
                        .unwrap_or_default(),
                ),
        )
    }

    pub fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.record_top_level(class_name);
        self.workspace
            .superclass_of(class_name)
            .or_else(|| self.base.superclass_of(class_name))
            .or_else(|| self.builtins.and_then(|b| b.superclass_of(class_name)))
            .or_else(|| self.virtual_object_base(class_name))
    }

    pub fn inherits_from(&self, class_name: &str, ancestor: &str) -> bool {
        self.try_in_chain(class_name, AccessLevel::Private, |current, _, _| {
            (current == ancestor).then_some(())
        })
        .is_some()
    }

    fn virtual_object_base(&self, class_name: &str) -> Option<String> {
        if OBJECT_ROOT_CHAIN.contains(&class_name) {
            return None;
        }
        let def = self.find_top_level(class_name)?;
        (def.symbol.kind == SymbolKind::Class).then(|| OBJECT_BASE_CLASS.to_string())
    }

    pub fn find_member(
        &self,
        container: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        let (lookup, element) = generic_lookup_target(container);
        let def = self.try_in_chain(lookup, min_access, |container, _depth, access| {
            self.record_member(container, name);
            self.workspace
                .direct_member_of(container, name, access)
                .or_else(|| self.base.direct_member_of(container, name, access))
                .or_else(|| {
                    self.builtins
                        .and_then(|b| b.direct_member_of(container, name, access))
                })
        });
        match (def, element) {
            (Some(d), Some(elem)) => Some(substitute_in_definition(d, container, elem)),
            (d, _) => d,
        }
    }

    pub fn direct_members_of(
        &self,
        container_name: &str,
        min_access: AccessLevel,
    ) -> Vec<Definition> {
        let (lookup, element) = generic_lookup_target(container_name);
        let raw = dedup_by_name(
            self.workspace
                .direct_members_of(lookup, min_access)
                .into_iter()
                .chain(self.base.direct_members_of(lookup, min_access))
                .chain(
                    self.builtins
                        .map(|b| b.direct_members_of(lookup, min_access))
                        .unwrap_or_default(),
                ),
        );
        match element {
            Some(elem) => raw
                .into_iter()
                .map(|d| substitute_in_definition(d, container_name, elem))
                .collect(),
            None => raw,
        }
    }

    pub fn members_of(&self, container: &str, min_access: AccessLevel) -> Vec<Definition> {
        self.members_of_tiered(container, min_access)
            .into_iter()
            .map(|(_, def)| def)
            .collect()
    }

    pub fn members_of_tiered(
        &self,
        container: &str,
        min_access: AccessLevel,
    ) -> Vec<(u8, Definition)> {
        let (lookup, element) = generic_lookup_target(container);
        let mut seen: HashMap<String, (u8, Definition)> = HashMap::new();
        self.try_in_chain::<(), _>(lookup, min_access, |c, depth, access| {
            let tier = if depth == 0 { 0u8 } else { 1u8 };
            for def in self
                .workspace
                .direct_members_of(c, access)
                .into_iter()
                .chain(self.base.direct_members_of(c, access))
                .chain(
                    self.builtins
                        .map(|b| b.direct_members_of(c, access))
                        .unwrap_or_default(),
                )
            {
                seen.entry(def.symbol.name.clone()).or_insert((tier, def));
            }
            None
        });
        match element {
            Some(elem) => seen
                .into_values()
                .map(|(t, d)| (t, substitute_in_definition(d, container, elem)))
                .collect(),
            None => seen.into_values().collect(),
        }
    }

    /// Class-body declaration first, then annotation declarations.
    pub(super) fn all_member_declarations(&self, container: &str, name: &str) -> Vec<Definition> {
        let mut decls: Vec<Definition> = Vec::new();
        if let Some(class_body) = self.find_member(container, name, AccessLevel::Private) {
            decls.push(class_body);
        }
        for def in self
            .workspace
            .annotated_members(container, name)
            .into_iter()
            .chain(self.base.annotated_members(container, name))
            .chain(
                self.builtins
                    .map(|b| b.annotated_members(container, name))
                    .unwrap_or_default(),
            )
        {
            decls.push(def);
        }
        dedup_definitions(decls)
    }

    fn try_in_chain<T, F>(&self, start: &str, min_access: AccessLevel, mut visit: F) -> Option<T>
    where
        F: FnMut(&str, usize, AccessLevel) -> Option<T>,
    {
        let mut current: String = start.to_string();
        let mut depth: usize = 0;
        let mut access = min_access;
        loop {
            if depth > MAX_INHERITANCE_DEPTH {
                return None;
            }
            self.record_top_level(&current);
            if let Some(found) = visit(&current, depth, access) {
                return Some(found);
            }
            let superclass = self.superclass_of(&current)?;
            depth += 1;
            access = access.max(AccessLevel::Protected);
            current = superclass;
        }
    }

    pub fn all_types(&self) -> Vec<Definition> {
        dedup_by_name(
            self.workspace
                .all_types()
                .into_iter()
                .chain(self.base.all_types())
                .chain(
                    self.builtins
                        .map(|b| b.all_types())
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|d| !crate::builtins::is_non_type_builtin(&d.uri)),
                ),
        )
    }

    pub fn all_top_level_callables(&self) -> Vec<Definition> {
        dedup_by_name(
            self.workspace
                .all_top_level_callables()
                .into_iter()
                .chain(self.base.all_top_level_callables()),
        )
    }

    pub fn all_script_globals(&self) -> Vec<Definition> {
        self.script_env
            .map(|env| {
                env.globals
                    .iter()
                    .map(|g| Definition {
                        uri: g.ini_uri.clone(),
                        symbol: g.symbol.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<String> {
        let params = self.workspace.parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        let params = self.base.parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        self.builtins
            .map(|b| b.parameters_of(uri, callable_id))
            .unwrap_or_default()
    }

    pub fn full_parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<Symbol> {
        let params = self.workspace.full_parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        let params = self.base.full_parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        self.builtins
            .map(|b| b.full_parameters_of(uri, callable_id))
            .unwrap_or_default()
    }
}

fn generic_lookup_target(container: &str) -> (&str, Option<&str>) {
    match super::parse_generic_type(container) {
        Some((ctor, elem)) => (ctor, Some(elem)),
        None => (container, None),
    }
}

fn substitute_placeholder(s: &str, placeholder: &str, replacement: &str) -> String {
    let bytes = s.as_bytes();
    let plen = placeholder.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(placeholder.as_bytes()) {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_idx = i + plen;
            let after_ok = after_idx >= bytes.len() || !is_ident_byte(bytes[after_idx]);
            if before_ok && after_ok {
                out.push_str(replacement);
                i += plen;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn substitute_in_definition(
    mut def: Definition,
    container_instance: &str,
    element: &str,
) -> Definition {
    let p = crate::builtins::GENERIC_ELEMENT_PLACEHOLDER;
    if let Some(t) = def.symbol.type_annotation.take() {
        def.symbol.type_annotation = Some(substitute_placeholder(&t, p, element));
    }
    if let Some(s) = def.symbol.signature.take() {
        def.symbol.signature = Some(substitute_placeholder(&s, p, element));
    }
    if def.symbol.container_name.is_some() {
        def.symbol.container_name = Some(container_instance.to_string());
    }
    def
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    documents: HashMap<String, Vec<Symbol>>,
    top_level_by_name: HashMap<String, Definition>,
    enum_variant_by_name: HashMap<String, Definition>,
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
    enum_variant_subscribers: HashMap<String, HashSet<String>>,
    subscriber_keys: HashMap<String, ObservationSet>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObservedKey {
    TopLevel(String),
    Member(String, String),
    EnumVariant(String),
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
        invalidated
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
        for name in &observations.enum_variants {
            self.enum_variant_subscribers
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
        for name in prev.enum_variants {
            if let Some(set) = self.enum_variant_subscribers.get_mut(&name) {
                set.remove(subscriber_uri);
                if set.is_empty() {
                    self.enum_variant_subscribers.remove(&name);
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
                ObservedKey::EnumVariant(n) => self.enum_variant_subscribers.get(n),
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
                }
                if is_type_like(sym.kind) {
                    self.superclass_by_name.remove(&sym.name);
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
                if let Some(members) = self.member_by_type.get_mut(cn) {
                    if members
                        .get(&sym.name)
                        .map(|d| d.uri == uri)
                        .unwrap_or(false)
                    {
                        members.remove(&sym.name);
                    }
                    if members.is_empty() {
                        self.member_by_type.remove(cn);
                    }
                }
                if sym.kind == SymbolKind::EnumVariant
                    && self
                        .enum_variant_by_name
                        .get(&sym.name)
                        .map(|d| d.uri == uri)
                        .unwrap_or(false)
                {
                    self.enum_variant_by_name.remove(&sym.name);
                }
            }
        }
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
                if sym.kind == SymbolKind::EnumVariant {
                    self.enum_variant_by_name.insert(
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

    pub fn find_enum_variant(&self, name: &str) -> Option<Definition> {
        self.enum_variant_by_name.get(name).cloned()
    }

    pub fn all_enum_variants(&self) -> Vec<Definition> {
        self.enum_variant_by_name.values().cloned().collect()
    }

    pub fn all_types(&self) -> Vec<Definition> {
        self.top_level_by_name
            .values()
            .filter(|d| is_type_like(d.symbol.kind) || d.symbol.kind == SymbolKind::Enum)
            .cloned()
            .collect()
    }

    pub fn all_top_level_callables(&self) -> Vec<Definition> {
        self.top_level_by_name
            .values()
            .filter(|d| {
                matches!(d.symbol.kind, SymbolKind::Function | SymbolKind::Event)
                    && !matches!(d.symbol.flavour.as_deref(), Some("exec") | Some("quest"))
            })
            .cloned()
            .collect()
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

    fn annotated_members(&self, container_name: &str, name: &str) -> Vec<Definition> {
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
