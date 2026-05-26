use std::collections::HashMap;

use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

use super::super::completion_catalog::{merge_ws_base, merge_ws_base_three};
use super::super::{dedup_by_name, dedup_definitions, MAX_INHERITANCE_DEPTH};
use super::generics::{generic_lookup_target, substitute_in_definition};
use super::SymbolDb;
use crate::resolve::Definition;

const OBJECT_BASE_CLASS: &str = "CObject";
const STATE_BASE_CLASS: &str = "CScriptableState";
const OBJECT_ROOT_CHAIN: [&str; 3] = ["CObject", "IScriptable", "ISerializable"];

impl<'a> SymbolDb<'a> {
    pub(crate) fn find_script_global(&self, name: &str) -> Option<Definition> {
        let g = self.script_env?.find(name)?;
        if let Some(class_def) = self.find_top_level(&g.type_name) {
            return Some(class_def);
        }
        Some(Definition {
            uri: g.ini_uri.clone(),
            symbol: g.symbol.clone(),
        })
    }

    pub(crate) fn script_global_type(&self, name: &str) -> Option<String> {
        self.script_env?.find(name).map(|g| g.type_name.clone())
    }

    pub fn find_top_level(&self, name: &str) -> Option<Definition> {
        self.record_top_level(name);
        self.workspace
            .find_top_level(name)
            .or_else(|| self.shadowed_base().find_top_level(name))
            .or_else(|| self.builtins.and_then(|b| b.find_top_level(name)))
    }

    pub fn find_enum_member(&self, name: &str) -> Option<Definition> {
        self.record_enum_member(name);
        self.workspace
            .find_enum_member(name)
            .or_else(|| self.shadowed_base().find_enum_member(name))
            .or_else(|| self.builtins.and_then(|b| b.find_enum_member(name)))
    }

    pub fn all_enum_members(&self) -> Vec<Definition> {
        self.merged_enum_members_catalog().iter().cloned().collect()
    }

    pub fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.record_top_level(class_name);
        self.workspace
            .superclass_of(class_name)
            .or_else(|| self.shadowed_base().superclass_of(class_name))
            .or_else(|| self.builtins.and_then(|b| b.superclass_of(class_name)))
            .or_else(|| self.virtual_object_base(class_name))
    }

    pub fn inherits_from(&self, class_name: &str, ancestor: &str) -> bool {
        self.try_in_chain(class_name, |current, _| (current == ancestor).then_some(()))
            .is_some()
    }

    fn virtual_object_base(&self, class_name: &str) -> Option<String> {
        if OBJECT_ROOT_CHAIN.contains(&class_name) {
            return None;
        }
        let def = self.find_top_level(class_name)?;
        match def.symbol.kind {
            SymbolKind::Class => Some(OBJECT_BASE_CLASS.to_string()),
            SymbolKind::State if class_name != STATE_BASE_CLASS => {
                Some(STATE_BASE_CLASS.to_string())
            }
            _ => None,
        }
    }

    pub fn find_member(
        &self,
        container: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        let (lookup, element) = generic_lookup_target(container);
        let def = self.try_in_chain(lookup, |container, _depth| {
            self.record_member(container, name);
            self.workspace
                .direct_member_of(container, name, AccessLevel::Private)
                .or_else(|| {
                    self.shadowed_base()
                        .direct_member_of(container, name, AccessLevel::Private)
                })
                .or_else(|| {
                    self.builtins
                        .and_then(|b| b.direct_member_of(container, name, AccessLevel::Private))
                })
        })?;
        if def.symbol.access < min_access {
            return None;
        }
        Some(match element {
            Some(elem) => substitute_in_definition(def, container, elem),
            None => def,
        })
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
                .chain(self.shadowed_base().direct_members_of(lookup, min_access))
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
        self.try_in_chain::<(), _>(lookup, |c, depth| {
            let tier = if depth == 0 { 0u8 } else { 1u8 };
            for def in self
                .workspace
                .direct_members_of(c, AccessLevel::Private)
                .into_iter()
                .chain(
                    self.shadowed_base()
                        .direct_members_of(c, AccessLevel::Private),
                )
                .chain(
                    self.builtins
                        .map(|b| b.direct_members_of(c, AccessLevel::Private))
                        .unwrap_or_default(),
                )
            {
                seen.entry(def.symbol.name.clone()).or_insert((tier, def));
            }
            None
        });
        seen.into_values()
            .filter(|(_, def)| def.symbol.access >= min_access)
            .map(|(t, d)| match element {
                Some(elem) => (t, substitute_in_definition(d, container, elem)),
                None => (t, d),
            })
            .collect()
    }

    /// Class-body declaration first, then annotation declarations.
    pub(crate) fn all_member_declarations(&self, container: &str, name: &str) -> Vec<Definition> {
        let mut decls: Vec<Definition> = Vec::new();
        if let Some(class_body) = self.find_member(container, name, AccessLevel::Private) {
            decls.push(class_body);
        }
        for def in self
            .workspace
            .annotated_members(container, name)
            .into_iter()
            .chain(self.shadowed_base().annotated_members(container, name))
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

    fn try_in_chain<T, F>(&self, start: &str, mut visit: F) -> Option<T>
    where
        F: FnMut(&str, usize) -> Option<T>,
    {
        let mut current: String = start.to_string();
        let mut depth: usize = 0;
        loop {
            if depth > MAX_INHERITANCE_DEPTH {
                return None;
            }
            self.record_top_level(&current);
            if let Some(found) = visit(&current, depth) {
                return Some(found);
            }
            let superclass = self.superclass_of(&current)?;
            depth += 1;
            current = superclass;
        }
    }

    pub fn all_types(&self) -> Vec<Definition> {
        self.merged_types_catalog().iter().cloned().collect()
    }

    pub fn all_top_level_callables(&self) -> Vec<Definition> {
        self.merged_callables_catalog().iter().cloned().collect()
    }

    pub fn merged_callables_catalog(&self) -> std::sync::Arc<[Definition]> {
        merge_ws_base(
            self.workspace.callables_catalog(),
            self.base_callables_for_merge(),
        )
    }

    pub fn merged_types_catalog(&self) -> std::sync::Arc<[Definition]> {
        let ws = self.workspace.types_catalog();
        let base = self.base_types_for_merge();
        match self.builtins {
            Some(_) => merge_ws_base_three(ws, base, crate::builtins::types_completion_catalog()),
            None => merge_ws_base(ws, base),
        }
    }

    pub fn merged_enum_members_catalog(&self) -> std::sync::Arc<[Definition]> {
        let ws = self.workspace.enum_members_catalog();
        let base = self.base_enum_members_for_merge();
        match self.builtins {
            Some(b) => merge_ws_base_three(ws, base, b.enum_members_catalog()),
            None => merge_ws_base(ws, base),
        }
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
        let params = self.shadowed_base().parameters_of(uri, callable_id);
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
        let params = self.shadowed_base().full_parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        self.builtins
            .map(|b| b.full_parameters_of(uri, callable_id))
            .unwrap_or_default()
    }
}
