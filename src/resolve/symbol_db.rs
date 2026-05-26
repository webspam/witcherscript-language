use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::script_env::ScriptEnvironment;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

use super::completion_catalog::{merge_ws_base, merge_ws_base_three};
use super::workspace_index::WorkspaceIndex;
use super::{dedup_by_name, dedup_definitions, Definition, ObservationSet, MAX_INHERITANCE_DEPTH};

const OBJECT_BASE_CLASS: &str = "CObject";
const STATE_BASE_CLASS: &str = "CScriptableState";

// These sit at/above CObject; a virtual CObject base would form a cycle.
const OBJECT_ROOT_CHAIN: [&str; 3] = ["CObject", "IScriptable", "ISerializable"];

pub struct SymbolDb<'a> {
    pub(super) workspace: &'a WorkspaceIndex,
    pub(super) base: &'a WorkspaceIndex,
    builtins: Option<&'a WorkspaceIndex>,
    script_env: Option<&'a ScriptEnvironment>,
    observer: Option<&'a Mutex<ObservationSet>>,
    suppressed_base_uris: Option<&'a HashSet<String>>,
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
        }
    }

    pub fn with_suppressed_base_uris(mut self, uris: &'a HashSet<String>) -> Self {
        self.suppressed_base_uris = Some(uris);
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
        }
    }

    pub(super) fn suppressed_base_uris(&self) -> Option<&HashSet<String>> {
        self.suppressed_base_uris
    }

    fn base_def_allowed(&self, def: &Definition) -> bool {
        match self.suppressed_base_uris {
            Some(sup) => !sup.contains(def.uri.as_str()),
            None => true,
        }
    }

    fn filter_base_catalog(
        catalog: std::sync::Arc<[Definition]>,
        suppressed: Option<&HashSet<String>>,
    ) -> std::sync::Arc<[Definition]> {
        let Some(sup) = suppressed else {
            return catalog;
        };
        let filtered: Vec<Definition> = catalog
            .iter()
            .filter(|d| !sup.contains(d.uri.as_str()))
            .cloned()
            .collect();
        Arc::from(filtered)
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
            .or_else(|| {
                self.base
                    .find_top_level(name)
                    .filter(|d| self.base_def_allowed(d))
            })
            .or_else(|| self.builtins.and_then(|b| b.find_top_level(name)))
    }

    pub fn find_enum_variant(&self, name: &str) -> Option<Definition> {
        self.record_enum_variant(name);
        self.workspace
            .find_enum_variant(name)
            .or_else(|| {
                self.base
                    .find_enum_variant(name)
                    .filter(|d| self.base_def_allowed(d))
            })
            .or_else(|| self.builtins.and_then(|b| b.find_enum_variant(name)))
    }

    pub fn all_enum_variants(&self) -> Vec<Definition> {
        self.merged_enum_variants_catalog()
            .iter()
            .cloned()
            .collect()
    }

    pub fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.record_top_level(class_name);
        self.workspace
            .superclass_of(class_name)
            .or_else(|| {
                if self
                    .base
                    .find_top_level(class_name)
                    .is_some_and(|d| !self.base_def_allowed(&d))
                {
                    return None;
                }
                self.base.superclass_of(class_name)
            })
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
                    self.base
                        .direct_member_of(container, name, AccessLevel::Private)
                        .filter(|d| self.base_def_allowed(d))
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
                .chain(
                    self.base
                        .direct_members_of(lookup, min_access)
                        .into_iter()
                        .filter(|d| self.base_def_allowed(d)),
                )
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
                    self.base
                        .direct_members_of(c, AccessLevel::Private)
                        .into_iter()
                        .filter(|d| self.base_def_allowed(d)),
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
    pub(super) fn all_member_declarations(&self, container: &str, name: &str) -> Vec<Definition> {
        let mut decls: Vec<Definition> = Vec::new();
        if let Some(class_body) = self.find_member(container, name, AccessLevel::Private) {
            decls.push(class_body);
        }
        for def in self
            .workspace
            .annotated_members(container, name)
            .into_iter()
            .chain(
                self.base
                    .annotated_members(container, name)
                    .into_iter()
                    .filter(|d| self.base_def_allowed(d)),
            )
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
            Self::filter_base_catalog(self.base.callables_catalog(), self.suppressed_base_uris),
        )
    }

    pub fn merged_types_catalog(&self) -> std::sync::Arc<[Definition]> {
        let ws = self.workspace.types_catalog();
        let base = Self::filter_base_catalog(self.base.types_catalog(), self.suppressed_base_uris);
        match self.builtins {
            Some(_) => merge_ws_base_three(ws, base, crate::builtins::types_completion_catalog()),
            None => merge_ws_base(ws, base),
        }
    }

    pub fn merged_enum_variants_catalog(&self) -> std::sync::Arc<[Definition]> {
        let ws = self.workspace.enum_variants_catalog();
        let base =
            Self::filter_base_catalog(self.base.enum_variants_catalog(), self.suppressed_base_uris);
        match self.builtins {
            Some(b) => merge_ws_base_three(ws, base, b.enum_variants_catalog()),
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
