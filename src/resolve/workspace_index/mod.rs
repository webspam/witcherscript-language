mod indices;
mod lookup;
mod subscribers;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::document::ParsedDocument;
use crate::symbols::Symbol;

use super::completion_catalog::{
    build_callables, build_enum_members, build_types, global_catalog_changed, CompletionCatalog,
};
use super::{Definition, ObservationSet};

pub use subscribers::ObservedKey;

use subscribers::{diff_outward_keys, doc_surface_hash, outward_hash_map, scan_ident_occurrences};

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
}
