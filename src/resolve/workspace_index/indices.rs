use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

use crate::symbols::{Symbol, SymbolKind};

use super::super::annotation_target_class;
use super::super::ast::is_type_like;
use super::super::state_classes::state_backing_class_name;
use super::{Definition, WorkspaceIndex};

fn retain_and_prune<K, V, Q>(
    map: &mut HashMap<K, Vec<V>>,
    key: &Q,
    mut keep: impl FnMut(&V) -> bool,
) where
    K: Hash + Eq + Borrow<Q>,
    Q: Hash + Eq + ?Sized,
{
    let Some(entries) = map.get_mut(key) else {
        return;
    };
    entries.retain(|v| keep(v));
    if entries.is_empty() {
        map.remove(key);
    }
}

fn retain_and_prune_nested<K1, K2, V, Q1>(
    map: &mut HashMap<K1, HashMap<K2, Vec<V>>>,
    outer_key: &Q1,
    inner_key: &K2,
    keep: impl FnMut(&V) -> bool,
) where
    K1: Hash + Eq + Borrow<Q1>,
    K2: Hash + Eq,
    Q1: Hash + Eq + ?Sized,
{
    let Some(inner) = map.get_mut(outer_key) else {
        return;
    };
    retain_and_prune(inner, inner_key, keep);
    if inner.is_empty() {
        map.remove(outer_key);
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
                        retain_and_prune_nested(&mut self.states_by_owner, owner, &sym.name, |d| {
                            d.uri != uri
                        });
                        let still_declared = self
                            .states_by_owner
                            .get(owner)
                            .is_some_and(|states| states.contains_key(&sym.name));
                        if !still_declared {
                            self.state_backing_by_name
                                .remove(&state_backing_class_name(owner, &sym.name));
                        }
                    }
                }
                if matches!(sym.kind, SymbolKind::Function | SymbolKind::Field) {
                    if let Some(target) = annotation_target_class(&sym) {
                        retain_and_prune_nested(
                            &mut self.annotated_members_by_type,
                            target,
                            &sym.name,
                            |d| d.uri != uri,
                        );
                    }
                }
            } else if let Some(cn) = &sym.container_name {
                retain_and_prune_nested(&mut self.member_by_type, cn, &sym.name, |d| d.uri != uri);
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
                        self.state_backing_by_name
                            .entry(state_backing_class_name(owner, &sym.name))
                            .or_insert_with(|| (owner.clone(), sym.name.clone()));
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
