mod indices;
mod lookup;
mod subscribers;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::document::ParsedDocument;
use crate::symbols::Symbol;

use super::Definition;
use super::completion_catalog::{
    CompletionCatalog, build_callables, build_enum_members, build_types, global_catalog_changed,
};

pub use subscribers::ObservedKey;

use subscribers::{diff_outward_keys, doc_surface_hash, outward_hash_map, scan_ident_occurrences};

// rough empirical estimate of bytes per HashMap entry slot; not authoritative.
const HASHMAP_ENTRY_BYTES: usize = 56;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    documents: HashMap<String, Vec<Symbol>>,
    top_level_by_name: HashMap<String, Vec<Definition>>,
    enum_member_by_name: HashMap<String, Vec<Definition>>,
    superclass_by_name: HashMap<String, Vec<(String, String)>>,
    // Reverse direction, needed because virtualParent dispatches down to subclasses.
    subclasses_by_name: HashMap<String, Vec<(String, String)>>,
    states_by_owner: HashMap<String, HashMap<String, Vec<Definition>>>,
    // Forward index: the synthetic name `OwnerStateS` cannot be reverse-split into owner + state.
    state_backing_by_name: HashMap<String, (String, String)>,
    member_by_type: HashMap<String, HashMap<String, Vec<Definition>>>,
    annotated_members_by_type: HashMap<String, HashMap<String, Vec<Definition>>>,
    doc_idents: HashMap<String, HashMap<String, Vec<Range<usize>>>>,
    doc_surface_hashes: HashMap<String, u64>,
    surface_hash: u64,
    generation: u64,
    doc_outward_hashes: HashMap<String, HashMap<ObservedKey, u64>>,
    completion_catalog: CompletionCatalog,
    completion_catalog_dirty: bool,
    catalog_rebuild_suppressed: u32,
}

pub struct DocContribution {
    symbols: Vec<Symbol>,
    idents: HashMap<String, Vec<Range<usize>>>,
    surface_hash: u64,
    outward: HashMap<ObservedKey, u64>,
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
    ) -> Vec<ObservedKey> {
        let uri: String = uri.into();
        let contribution = Self::compute_contribution(&uri, document);
        self.apply_contribution(uri, contribution)
    }

    /// Pure half of `update_document`, so callers can run it in parallel before a serial apply.
    pub fn compute_contribution(uri: &str, document: &ParsedDocument) -> DocContribution {
        let symbols = document.symbols.all().to_vec();
        let idents = scan_ident_occurrences(document);
        let surface_hash = doc_surface_hash(uri, &symbols);
        let outward = outward_hash_map(&symbols);
        DocContribution {
            symbols,
            idents,
            surface_hash,
            outward,
        }
    }

    /// Mutating half of `update_document`: folds a precomputed contribution into the index.
    pub fn apply_contribution(
        &mut self,
        uri: impl Into<String>,
        contribution: DocContribution,
    ) -> Vec<ObservedKey> {
        let uri: String = uri.into();
        let DocContribution {
            symbols,
            idents,
            surface_hash,
            outward,
        } = contribution;

        self.remove_from_indices(&uri);
        self.doc_idents.remove(&uri);
        self.insert_into_indices(&uri, &symbols);
        self.doc_idents.insert(uri.clone(), idents);

        if let Some(old_hash) = self.doc_surface_hashes.insert(uri.clone(), surface_hash) {
            self.surface_hash ^= old_hash;
        }
        self.surface_hash ^= surface_hash;

        let changed_keys = diff_outward_keys(self.doc_outward_hashes.get(&uri), &outward);
        self.doc_outward_hashes.insert(uri.clone(), outward);

        self.documents.insert(uri, symbols);
        self.generation = self.generation.wrapping_add(1);
        if global_catalog_changed(&changed_keys) {
            self.completion_catalog_dirty = true;
            self.maybe_rebuild_completion_catalog();
        }
        changed_keys
    }

    pub fn remove_document(&mut self, uri: &str) -> Vec<ObservedKey> {
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
        self.generation = self.generation.wrapping_add(1);
        if global_catalog_changed(&changed_keys) {
            self.completion_catalog_dirty = true;
            self.maybe_rebuild_completion_catalog();
        }
        changed_keys
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
                total += ranges.capacity() * size_of::<Range<usize>>();
            }
            total += name_map.capacity() * HASHMAP_ENTRY_BYTES;
        }
        total += self.doc_idents.capacity() * HASHMAP_ENTRY_BYTES;
        total
    }

    pub(super) fn ident_ranges_in_doc(&self, uri: &str, name: &str) -> Option<&[Range<usize>]> {
        self.doc_idents.get(uri)?.get(name).map(Vec::as_slice)
    }
}
