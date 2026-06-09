use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

use super::super::NameContext;
use super::super::state_classes::StateBackingClass;
use super::{Definition, WorkspaceIndex};

impl WorkspaceIndex {
    /// First match for `name`, preferring a non-state kind so callers that
    /// only want one definition (e.g. `this` resolution, hover) keep working
    /// when a same-named function or class exists alongside the state.
    pub fn find_top_level(&self, name: &str) -> Option<Definition> {
        let defs = self.top_level_by_name.get(name)?;
        defs.iter()
            .find(|d| d.symbol.kind != SymbolKind::State)
            .or_else(|| defs.first())
            .cloned()
    }

    /// First match whose kind is accepted by `ctx`.
    pub fn find_top_level_filtered(&self, name: &str, ctx: &NameContext) -> Option<Definition> {
        self.top_level_by_name
            .get(name)?
            .iter()
            .find(|d| ctx.accepts(d.symbol.kind))
            .cloned()
    }

    pub fn all_top_level_with_name(&self, name: &str) -> &[Definition] {
        self.top_level_by_name.get(name).map_or(&[], Vec::as_slice)
    }

    pub fn find_state_in_owner(&self, owner: &str, name: &str) -> Option<Definition> {
        self.states_by_owner.get(owner)?.get(name)?.last().cloned()
    }

    pub fn has_state_named(&self, name: &str) -> bool {
        self.states_by_owner
            .values()
            .any(|states| states.contains_key(name))
    }

    pub(crate) fn find_state_backing_class(&self, name: &str) -> Option<StateBackingClass<'_>> {
        let (synthetic, (owner, state)) = self.state_backing_by_name.get_key_value(name)?;
        let declaration = self.states_by_owner.get(owner)?.get(state)?.last()?;
        Some(StateBackingClass::new(synthetic, declaration))
    }

    pub fn find_enum_member(&self, name: &str) -> Option<Definition> {
        self.enum_member_by_name.get(name)?.last().cloned()
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

    /// Unlike `top_level_by_name`, does not dedup by name - name collisions stay visible.
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
            .and_then(|defs| defs.last())
            .or_else(|| {
                self.annotated_members_by_type
                    .get(container_name)
                    .and_then(|members| members.get(name))
                    .and_then(|defs| defs.first())
            })
            .filter(|def| def.symbol.access >= min_access)
            .cloned()
    }

    // Class-body declarations only, never annotation overlays: the method a `@wrapMethod` wraps.
    pub(crate) fn class_body_member_of(
        &self,
        container_name: &str,
        name: &str,
    ) -> Option<Definition> {
        self.member_by_type
            .get(container_name)
            .and_then(|members| members.get(name))
            .and_then(|defs| defs.last())
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
            .flat_map(|m| m.values().filter_map(|v| v.last().cloned()));
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

    pub(crate) fn annotated_members(&self, container_name: &str, name: &str) -> Vec<Definition> {
        self.annotated_members_by_type
            .get(container_name)
            .and_then(|m| m.get(name))
            .cloned()
            .unwrap_or_default()
    }

    pub fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.superclass_by_name
            .get(class_name)?
            .last()
            .map(|(_, base)| base.clone())
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
