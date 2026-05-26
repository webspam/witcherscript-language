use crate::symbols::{Symbol, SymbolKind};

use super::super::annotation_target_class;
use super::super::ast::is_type_like;
use super::{Definition, WorkspaceIndex};

impl WorkspaceIndex {
    pub(super) fn remove_from_indices(&mut self, uri: &str) {
        let Some(old_symbols) = self.documents.get(uri) else {
            return;
        };
        for sym in old_symbols.clone() {
            if sym.container.is_none() {
                if self
                    .top_level_by_name
                    .get(&sym.name)
                    .map(|d| d.uri == uri)
                    .unwrap_or(false)
                {
                    self.top_level_by_name.remove(&sym.name);
                    if let Some(def) = self
                        .find_replacement_def(uri, |s| s.container.is_none() && s.name == sym.name)
                    {
                        self.top_level_by_name.insert(sym.name.clone(), def);
                    }
                }
                if is_type_like(sym.kind) {
                    self.superclass_by_name.remove(&sym.name);
                    let base = self
                        .find_replacement_def(uri, |s| {
                            s.container.is_none() && is_type_like(s.kind) && s.name == sym.name
                        })
                        .and_then(|def| def.symbol.base_class);
                    if let Some(base) = base {
                        self.superclass_by_name.insert(sym.name.clone(), base);
                    }
                }
                if matches!(sym.kind, SymbolKind::Function | SymbolKind::Field) {
                    if let Some(target) = annotation_target_class(&sym) {
                        if let Some(by_name) = self.annotated_members_by_type.get_mut(target) {
                            if let Some(defs) = by_name.get_mut(&sym.name) {
                                defs.retain(|d| d.uri != uri);
                                if defs.is_empty() {
                                    by_name.remove(&sym.name);
                                }
                            }
                            if by_name.is_empty() {
                                self.annotated_members_by_type.remove(target);
                            }
                        }
                    }
                }
            } else if let Some(cn) = &sym.container_name {
                let owns_member = self
                    .member_by_type
                    .get(cn)
                    .and_then(|m| m.get(&sym.name))
                    .map(|d| d.uri == uri)
                    .unwrap_or(false);
                if owns_member {
                    let replacement = self.find_replacement_def(uri, |s| {
                        s.container_name.as_deref() == Some(cn.as_str()) && s.name == sym.name
                    });
                    let members = self.member_by_type.entry(cn.clone()).or_default();
                    members.remove(&sym.name);
                    if let Some(def) = replacement {
                        members.insert(sym.name.clone(), def);
                    }
                    if members.is_empty() {
                        self.member_by_type.remove(cn);
                    }
                }
                if sym.kind == SymbolKind::EnumMember
                    && self
                        .enum_member_by_name
                        .get(&sym.name)
                        .map(|d| d.uri == uri)
                        .unwrap_or(false)
                {
                    self.enum_member_by_name.remove(&sym.name);
                    if let Some(def) = self.find_replacement_def(uri, |s| {
                        s.kind == SymbolKind::EnumMember && s.name == sym.name
                    }) {
                        self.enum_member_by_name.insert(sym.name.clone(), def);
                    }
                }
            }
        }
    }

    fn find_replacement_def<F>(&self, exclude_uri: &str, predicate: F) -> Option<Definition>
    where
        F: Fn(&Symbol) -> bool,
    {
        self.documents
            .iter()
            .filter(|(other_uri, _)| other_uri.as_str() != exclude_uri)
            .find_map(|(other_uri, syms)| {
                syms.iter().find(|s| predicate(s)).map(|s| Definition {
                    uri: other_uri.clone(),
                    symbol: s.clone(),
                })
            })
    }

    pub(super) fn insert_into_indices(&mut self, uri: &str, symbols: &[Symbol]) {
        for sym in symbols {
            if sym.container.is_none() {
                self.top_level_by_name.insert(
                    sym.name.clone(),
                    Definition {
                        uri: uri.to_string(),
                        symbol: sym.clone(),
                    },
                );
                if is_type_like(sym.kind) {
                    if let Some(superclass) = &sym.base_class {
                        self.superclass_by_name
                            .insert(sym.name.clone(), superclass.clone());
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
                self.member_by_type.entry(cn.clone()).or_default().insert(
                    sym.name.clone(),
                    Definition {
                        uri: uri.to_string(),
                        symbol: sym.clone(),
                    },
                );
                if sym.kind == SymbolKind::EnumMember {
                    self.enum_member_by_name.insert(
                        sym.name.clone(),
                        Definition {
                            uri: uri.to_string(),
                            symbol: sym.clone(),
                        },
                    );
                }
            }
        }
    }
}
