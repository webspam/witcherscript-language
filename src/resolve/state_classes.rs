//! Source of truth for the engine-synthesised backing class of a state.
//!
//! A `state S in Owner [extends Base]` is compiled by the engine into a class
//! named `OwnerStateS` that has no declaration in source: its base is the
//! state's `extends` (or the implicit `CScriptableState`), its members are the
//! state's members, and `parent` inside it refers to `Owner`. This module names
//! that class and exposes a lightweight view over the state it derives from.

use crate::symbols::SymbolKind;

use super::Definition;

/// Engine-synthesised backing class name for `state {state} in {owner}`.
///
/// Owner + literal `State` + state name. The only place this naming convention
/// is encoded; every producer and consumer must agree on it.
pub(crate) fn state_backing_class_name(owner: &str, state: &str) -> String {
    format!("{owner}State{state}")
}

/// A view over the synthetic backing class of a single state declaration.
///
/// Borrows from the index it was looked up in; the live state `Definition`
/// stays single-sourced in `states_by_owner`.
#[derive(Debug, Clone, Copy)]
pub struct StateBackingClass<'a> {
    name: &'a str,
    owner: &'a str,
    declaration: &'a Definition,
}

impl<'a> StateBackingClass<'a> {
    pub(crate) fn new(name: &'a str, owner: &'a str, declaration: &'a Definition) -> Self {
        Self {
            name,
            owner,
            declaration,
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn owner_class(&self) -> &str {
        self.owner
    }

    pub fn state_name(&self) -> &str {
        &self.declaration.symbol.name
    }

    pub fn base_class(&self) -> Option<&str> {
        self.declaration.symbol.base_class.as_deref()
    }

    pub fn declaration(&self) -> &Definition {
        self.declaration
    }

    /// Base is the state itself, so the inheritance walk reaches the state's members.
    pub(crate) fn as_class_definition(&self) -> Definition {
        let mut symbol = self.declaration.symbol.clone();
        symbol.name = self.name.to_string();
        symbol.kind = SymbolKind::Class;
        symbol.base_class = Some(self.declaration.symbol.name.clone());
        symbol.owner_class = None;
        symbol.annotations = Vec::new();
        Definition {
            uri: self.declaration.uri.clone(),
            symbol,
        }
    }
}
