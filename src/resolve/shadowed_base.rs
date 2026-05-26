use std::collections::HashSet;
use std::sync::Arc;

use crate::symbols::{AccessLevel, Symbol, SymbolId};

use super::workspace_index::WorkspaceIndex;
use super::Definition;

pub(super) struct ShadowedBase<'a> {
    index: &'a WorkspaceIndex,
    suppressed: Option<&'a HashSet<String>>,
}

impl<'a> ShadowedBase<'a> {
    pub(super) fn new(index: &'a WorkspaceIndex, suppressed: Option<&'a HashSet<String>>) -> Self {
        Self { index, suppressed }
    }

    fn uri_visible(&self, uri: &str) -> bool {
        match self.suppressed {
            Some(sup) => !sup.contains(uri),
            None => true,
        }
    }

    fn def_visible(&self, def: &Definition) -> bool {
        self.uri_visible(def.uri.as_str())
    }

    pub(super) fn find_top_level(&self, name: &str) -> Option<Definition> {
        self.index
            .find_top_level(name)
            .filter(|d| self.def_visible(d))
    }

    pub(super) fn find_enum_member(&self, name: &str) -> Option<Definition> {
        self.index
            .find_enum_member(name)
            .filter(|d| self.def_visible(d))
    }

    pub(super) fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.find_top_level(class_name)?;
        self.index.superclass_of(class_name)
    }

    pub(super) fn direct_member_of(
        &self,
        container: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        self.index
            .direct_member_of(container, name, min_access)
            .filter(|d| self.def_visible(d))
    }

    pub(super) fn direct_members_of(
        &self,
        container: &str,
        min_access: AccessLevel,
    ) -> Vec<Definition> {
        self.index
            .direct_members_of(container, min_access)
            .into_iter()
            .filter(|d| self.def_visible(d))
            .collect()
    }

    pub(super) fn annotated_members(&self, container: &str, name: &str) -> Vec<Definition> {
        self.index
            .annotated_members(container, name)
            .into_iter()
            .filter(|d| self.def_visible(d))
            .collect()
    }

    pub(super) fn callables_catalog(&self) -> Arc<[Definition]> {
        filter_catalog(self.index.callables_catalog(), self.suppressed)
    }

    pub(super) fn types_catalog(&self) -> Arc<[Definition]> {
        filter_catalog(self.index.types_catalog(), self.suppressed)
    }

    pub(super) fn enum_members_catalog(&self) -> Arc<[Definition]> {
        filter_catalog(self.index.enum_members_catalog(), self.suppressed)
    }

    pub(super) fn parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<String> {
        if !self.uri_visible(uri) {
            return Vec::new();
        }
        self.index.parameters_of(uri, callable_id)
    }

    pub(super) fn full_parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<Symbol> {
        if !self.uri_visible(uri) {
            return Vec::new();
        }
        self.index.full_parameters_of(uri, callable_id)
    }
}

pub(super) fn filter_catalog(
    catalog: Arc<[Definition]>,
    suppressed: Option<&HashSet<String>>,
) -> Arc<[Definition]> {
    let Some(sup) = suppressed else {
        return catalog;
    };
    if sup.is_empty() {
        return catalog;
    }
    let filtered: Vec<Definition> = catalog
        .iter()
        .filter(|d| !sup.contains(d.uri.as_str()))
        .cloned()
        .collect();
    Arc::from(filtered)
}
