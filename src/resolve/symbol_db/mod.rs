mod generics;
mod lookup;

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use super::shadowed_base::{filter_catalog, ShadowedBase};
use super::workspace_index::WorkspaceIndex;
use super::ObservationSet;
use crate::script_env::ScriptEnvironment;

pub struct SymbolDb<'a> {
    pub(super) workspace: &'a WorkspaceIndex,
    /// Full base index, including suppressed URIs (e.g. find-references in shadowed vanilla files).
    pub(super) base: &'a WorkspaceIndex,
    builtins: Option<&'a WorkspaceIndex>,
    script_env: Option<&'a ScriptEnvironment>,
    observer: Option<&'a Mutex<ObservationSet>>,
    suppressed_base_uris: Option<&'a HashSet<String>>,
    prefiltered_base: Option<&'a FilteredBaseCatalogs>,
}

#[derive(Debug)]
pub struct FilteredBaseCatalogs {
    pub callables: Arc<[super::Definition]>,
    pub types: Arc<[super::Definition]>,
    pub enum_members: Arc<[super::Definition]>,
}

impl FilteredBaseCatalogs {
    pub fn build(base: &WorkspaceIndex, suppressed: &HashSet<String>) -> Self {
        Self {
            callables: filter_catalog(base.callables_catalog(), Some(suppressed)),
            types: filter_catalog(base.types_catalog(), Some(suppressed)),
            enum_members: filter_catalog(base.enum_members_catalog(), Some(suppressed)),
        }
    }
}

impl<'a> SymbolDb<'a> {
    pub fn new(workspace: &'a WorkspaceIndex, base: &'a WorkspaceIndex) -> Self {
        Self {
            workspace,
            base,
            builtins: None,
            script_env: None,
            observer: None,
            suppressed_base_uris: None,
            prefiltered_base: None,
        }
    }

    pub fn with_suppressed_base_uris(mut self, uris: &'a HashSet<String>) -> Self {
        self.suppressed_base_uris = Some(uris);
        self
    }

    pub fn with_prefiltered_base(mut self, catalogs: &'a FilteredBaseCatalogs) -> Self {
        self.prefiltered_base = Some(catalogs);
        self
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
            suppressed_base_uris: self.suppressed_base_uris,
            prefiltered_base: self.prefiltered_base,
        }
    }

    pub(super) fn suppressed_base_uris(&self) -> Option<&HashSet<String>> {
        self.suppressed_base_uris
    }

    pub(super) fn shadowed_base(&self) -> ShadowedBase<'a> {
        ShadowedBase::new(self.base, self.suppressed_base_uris)
    }

    pub(super) fn base_callables_for_merge(&self) -> Arc<[super::Definition]> {
        if let Some(prefiltered) = self.prefiltered_base {
            return prefiltered.callables.clone();
        }
        self.shadowed_base().callables_catalog()
    }

    pub(super) fn base_types_for_merge(&self) -> Arc<[super::Definition]> {
        if let Some(prefiltered) = self.prefiltered_base {
            return prefiltered.types.clone();
        }
        self.shadowed_base().types_catalog()
    }

    pub(super) fn base_enum_members_for_merge(&self) -> Arc<[super::Definition]> {
        if let Some(prefiltered) = self.prefiltered_base {
            return prefiltered.enum_members.clone();
        }
        self.shadowed_base().enum_members_catalog()
    }

    pub fn merge_observations(&self, other: ObservationSet) {
        let Some(outer) = self.observer else {
            return;
        };
        let mut outer = outer.lock();
        outer.top_level.extend(other.top_level);
        outer.members.extend(other.members);
        outer.enum_members.extend(other.enum_members);
    }

    pub(super) fn record_top_level(&self, name: &str) {
        if let Some(obs) = self.observer {
            let mut o = obs.lock();
            if !o.top_level.contains(name) {
                o.top_level.insert(name.to_string());
            }
        }
    }

    pub(super) fn record_member(&self, container: &str, name: &str) {
        if let Some(obs) = self.observer {
            let mut o = obs.lock();
            let key = (container.to_string(), name.to_string());
            if !o.members.contains(&key) {
                o.members.insert(key);
            }
        }
    }

    pub(super) fn record_enum_member(&self, name: &str) {
        if let Some(obs) = self.observer {
            let mut o = obs.lock();
            if !o.enum_members.contains(name) {
                o.enum_members.insert(name.to_string());
            }
        }
    }
}
