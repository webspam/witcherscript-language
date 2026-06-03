//! Source of truth for the engine-synthesised backing class of a state.
//!
//! A `state S in Owner [extends Base]` is compiled by the engine into a class
//! named `OwnerStateS` that has no declaration in source: its base is the
//! state's `extends` (or the implicit `CScriptableState`), its members are the
//! state's members, and `parent` inside it refers to `Owner`. This module names
//! that class and exposes a lightweight view over the state it derives from.

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

    /// Synthetic class name, e.g. `OwnerStateS`.
    pub fn name(&self) -> &str {
        self.name
    }

    /// Owner class the state is declared `in` - target of `parent` and the
    /// subject of owner-exists checks.
    pub fn owner_class(&self) -> &str {
        self.owner
    }

    /// The state's own short name - the key its members live under in
    /// `member_by_type`, distinct from the synthetic class name.
    pub fn state_name(&self) -> &str {
        &self.declaration.symbol.name
    }

    /// The state's explicit `extends` base, if any. `None` means the engine's
    /// implicit `CScriptableState`, resolved by consumers rather than stored.
    pub fn base_class(&self) -> Option<&str> {
        self.declaration.symbol.base_class.as_deref()
    }

    /// The state declaration this backing class derives from - the go-to
    /// target and the source of the state's members.
    pub fn declaration(&self) -> &Definition {
        self.declaration
    }
}
