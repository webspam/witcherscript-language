use std::collections::HashSet;
use std::sync::Arc;

use crate::symbols::SymbolKind;

use super::workspace_index::ObservedKey;
use super::Definition;

#[derive(Debug, Clone, Default)]
pub struct CompletionCatalog {
    pub callables: Arc<[Definition]>,
    pub types: Arc<[Definition]>,
    pub enum_members: Arc<[Definition]>,
}

pub fn global_catalog_changed(keys: &[ObservedKey]) -> bool {
    keys.iter()
        .any(|k| matches!(k, ObservedKey::TopLevel(_) | ObservedKey::EnumMember(_)))
}

pub fn build_callables(
    top_level: &std::collections::HashMap<String, Vec<Definition>>,
) -> Vec<Definition> {
    top_level
        .values()
        .flat_map(|defs| defs.iter())
        .filter(|d| {
            matches!(d.symbol.kind, SymbolKind::Function)
                && !matches!(d.symbol.flavour.as_deref(), Some("exec") | Some("quest"))
        })
        .cloned()
        .collect()
}

pub fn build_types(
    top_level: &std::collections::HashMap<String, Vec<Definition>>,
) -> Vec<Definition> {
    top_level
        .values()
        .flat_map(|defs| defs.iter())
        .filter(|d| d.symbol.kind.is_type())
        .cloned()
        .collect()
}

pub fn build_enum_members(
    enum_member_by_name: &std::collections::HashMap<String, Vec<Definition>>,
) -> Vec<Definition> {
    enum_member_by_name
        .values()
        .filter_map(|defs| defs.last().cloned())
        .collect()
}

pub fn merge_ws_base(ws: Arc<[Definition]>, base: Arc<[Definition]>) -> Arc<[Definition]> {
    if base.is_empty() {
        return ws;
    }
    if ws.is_empty() {
        return base;
    }
    let shadowed: HashSet<&str> = ws.iter().map(|d| d.symbol.name.as_str()).collect();
    let extra: usize = base
        .iter()
        .filter(|d| !shadowed.contains(d.symbol.name.as_str()))
        .count();
    if extra == 0 {
        return ws;
    }
    let mut out = Vec::with_capacity(ws.len() + extra);
    out.extend(ws.iter().cloned());
    for def in base.iter() {
        if !shadowed.contains(def.symbol.name.as_str()) {
            out.push(def.clone());
        }
    }
    Arc::from(out)
}

pub fn merge_ws_base_three(
    ws: Arc<[Definition]>,
    base: Arc<[Definition]>,
    builtins: Arc<[Definition]>,
) -> Arc<[Definition]> {
    let mut shadowed = HashSet::with_capacity(ws.len());
    let mut out = Vec::with_capacity(ws.len() + base.len() + builtins.len());
    for def in ws.iter() {
        shadowed.insert(def.symbol.name.as_str());
        out.push(def.clone());
    }
    for def in base.iter().chain(builtins.iter()) {
        if shadowed.insert(def.symbol.name.as_str()) {
            out.push(def.clone());
        }
    }
    Arc::from(out)
}
