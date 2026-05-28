use std::collections::HashMap;
use std::hash::Hash;

use crate::symbols::{Symbol, SymbolKind};

use super::super::annotation_target_class;
use super::super::ast::is_type_like;
use super::{Definition, WorkspaceIndex};

fn retain_and_prune<K, V>(map: &mut HashMap<K, Vec<V>>, key: &K, mut keep: impl FnMut(&V) -> bool)
where
    K: Hash + Eq,
{
    let Some(entries) = map.get_mut(key) else {
        return;
    };
    entries.retain(|v| keep(v));
    if entries.is_empty() {
        map.remove(key);
    }
}

impl WorkspaceIndex {
    pub(super) fn remove_from_indices(&mut self, uri: &str) {
        let Some(old_symbols) = self.documents.get(uri) else {
            return;
        };
        for sym in old_symbols.clone() {
            if sym.container.is_none() {
                retain_and_prune(&mut self.top_level_by_name, &sym.name, |d| d.uri != uri);
                if is_type_like(sym.kind) {
                    retain_and_prune(&mut self.superclass_by_name, &sym.name, |(u, _)| u != uri);
                }
                if sym.kind == SymbolKind::State {
                    if let Some(owner) = &sym.owner_class {
                        if let Some(by_name) = self.states_by_owner.get_mut(owner) {
                            retain_and_prune(by_name, &sym.name, |d| d.uri != uri);
                            if by_name.is_empty() {
                                self.states_by_owner.remove(owner);
                            }
                        }
                    }
                }
                if matches!(sym.kind, SymbolKind::Function | SymbolKind::Field) {
                    if let Some(target) = annotation_target_class(&sym) {
                        if let Some(by_name) = self.annotated_members_by_type.get_mut(target) {
                            retain_and_prune(by_name, &sym.name, |d| d.uri != uri);
                            if by_name.is_empty() {
                                self.annotated_members_by_type.remove(target);
                            }
                        }
                    }
                }
            } else if let Some(cn) = &sym.container_name {
                if let Some(by_name) = self.member_by_type.get_mut(cn) {
                    retain_and_prune(by_name, &sym.name, |d| d.uri != uri);
                    if by_name.is_empty() {
                        self.member_by_type.remove(cn);
                    }
                }
                if sym.kind == SymbolKind::EnumMember {
                    retain_and_prune(&mut self.enum_member_by_name, &sym.name, |d| d.uri != uri);
                }
            }
        }
    }

    pub(super) fn insert_into_indices(&mut self, uri: &str, symbols: &[Symbol]) {
        for sym in symbols {
            if sym.container.is_none() {
                self.top_level_by_name
                    .entry(sym.name.clone())
                    .or_default()
                    .push(Definition {
                        uri: uri.to_string(),
                        symbol: sym.clone(),
                    });
                if is_type_like(sym.kind) {
                    if let Some(superclass) = &sym.base_class {
                        self.superclass_by_name
                            .entry(sym.name.clone())
                            .or_default()
                            .push((uri.to_string(), superclass.clone()));
                    }
                }
                if sym.kind == SymbolKind::State {
                    if let Some(owner) = &sym.owner_class {
                        self.states_by_owner
                            .entry(owner.clone())
                            .or_default()
                            .entry(sym.name.clone())
                            .or_default()
                            .push(Definition {
                                uri: uri.to_string(),
                                symbol: sym.clone(),
                            });
                    }
                }
                if matches!(sym.kind, SymbolKind::Function | SymbolKind::Field) {
                    if let Some(target) = annotation_target_class(sym) {
                        self.annotated_members_by_type
                            .entry(target.to_string())
                            .or_default()
                            .entry(sym.name.clone())
                            .or_default()
                            .push(Definition {
                                uri: uri.to_string(),
                                symbol: sym.clone(),
                            });
                    }
                }
            } else if let Some(cn) = &sym.container_name {
                self.member_by_type
                    .entry(cn.clone())
                    .or_default()
                    .entry(sym.name.clone())
                    .or_default()
                    .push(Definition {
                        uri: uri.to_string(),
                        symbol: sym.clone(),
                    });
                if sym.kind == SymbolKind::EnumMember {
                    self.enum_member_by_name
                        .entry(sym.name.clone())
                        .or_default()
                        .push(Definition {
                            uri: uri.to_string(),
                            symbol: sym.clone(),
                        });
                }
            }
        }
    }
}
