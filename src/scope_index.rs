use std::collections::HashMap;

use crate::symbols::{DocumentSymbols, SymbolId, SymbolKind};

#[derive(Debug, Clone, Default)]
pub struct DocScopeIndex {
    callable_spans: Vec<(usize, usize, SymbolId)>,
    type_spans: Vec<(usize, usize, SymbolId)>,
    locals_by_callable: HashMap<SymbolId, Vec<LocalEntry>>,
    top_level_by_name: HashMap<String, SymbolId>,
    type_like_by_name: HashMap<String, SymbolId>,
    members_by_container: HashMap<SymbolId, HashMap<String, SymbolId>>,
}

#[derive(Debug, Clone)]
pub struct LocalEntry {
    pub name: String,
    pub selection_start: usize,
    pub id: SymbolId,
}

impl DocScopeIndex {
    pub fn build(symbols: &DocumentSymbols) -> Self {
        let mut idx = Self::default();

        for s in symbols.all() {
            match s.kind {
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Event => {
                    idx.callable_spans
                        .push((s.byte_range.start, s.byte_range.end, s.id));
                }
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::State => {
                    idx.type_spans
                        .push((s.byte_range.start, s.byte_range.end, s.id));
                    if s.container.is_none() {
                        idx.type_like_by_name.entry(s.name.clone()).or_insert(s.id);
                    }
                }
                _ => {}
            }

            if s.container.is_none() {
                idx.top_level_by_name.entry(s.name.clone()).or_insert(s.id);
            } else if let Some(c) = s.container {
                if matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter) {
                    idx.locals_by_callable
                        .entry(c)
                        .or_default()
                        .push(LocalEntry {
                            name: s.name.clone(),
                            selection_start: s.selection_byte_range.start,
                            id: s.id,
                        });
                } else {
                    idx.members_by_container
                        .entry(c)
                        .or_default()
                        .entry(s.name.clone())
                        .or_insert(s.id);
                }
            }
        }

        idx.callable_spans
            .sort_unstable_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
        idx.type_spans
            .sort_unstable_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
        for v in idx.locals_by_callable.values_mut() {
            v.sort_unstable_by_key(|e| e.selection_start);
        }
        idx
    }

    pub fn enclosing_callable(&self, byte: usize) -> Option<SymbolId> {
        innermost_containing(&self.callable_spans, byte)
    }

    pub fn enclosing_type(&self, byte: usize) -> Option<SymbolId> {
        innermost_containing(&self.type_spans, byte)
    }

    pub fn local_or_param_at(
        &self,
        callable: SymbolId,
        name: &str,
        byte: usize,
    ) -> Option<SymbolId> {
        let entries = self.locals_by_callable.get(&callable)?;
        entries
            .iter()
            .rev()
            .find(|e| e.name == name && e.selection_start <= byte)
            .map(|e| e.id)
    }

    pub fn locals_in(&self, callable: SymbolId) -> &[LocalEntry] {
        self.locals_by_callable
            .get(&callable)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn top_level(&self, name: &str) -> Option<SymbolId> {
        self.top_level_by_name.get(name).copied()
    }

    pub fn type_like_container(&self, name: &str) -> Option<SymbolId> {
        self.type_like_by_name.get(name).copied()
    }

    pub fn direct_member(&self, container: SymbolId, name: &str) -> Option<SymbolId> {
        self.members_by_container
            .get(&container)
            .and_then(|m| m.get(name))
            .copied()
    }
}

fn innermost_containing(spans: &[(usize, usize, SymbolId)], byte: usize) -> Option<SymbolId> {
    let upper = spans.partition_point(|(start, _, _)| *start <= byte);
    for (_, end, id) in spans[..upper].iter().rev() {
        if byte <= *end {
            return Some(*id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::document::parse_document;
    use crate::symbols::SymbolKind;

    fn byte_of(source: &str, needle: &str) -> usize {
        source.find(needle).expect("needle present in source")
    }

    #[test]
    fn enclosing_callable_returns_innermost() {
        struct Case {
            name: &'static str,
            needle: &'static str,
            expected: &'static str,
        }
        let source = "class C { function F() { var x : int; } function G() { var y : int; } }\n";
        let doc = parse_document(source).unwrap();
        let cases = [
            Case {
                name: "inside F",
                needle: "var x",
                expected: "F",
            },
            Case {
                name: "inside G",
                needle: "var y",
                expected: "G",
            },
        ];
        for c in cases {
            let id = doc
                .scope_index
                .enclosing_callable(byte_of(source, c.needle))
                .unwrap_or_else(|| panic!("case {}: no enclosing callable", c.name));
            let sym = doc.symbols.by_id(id).unwrap();
            assert_eq!(sym.name, c.expected, "case {}", c.name);
        }
    }

    #[test]
    fn enclosing_type_returns_innermost_top_level() {
        let source = "class A { function F() {} } class B { function G() {} }\n";
        let doc = parse_document(source).unwrap();
        let id = doc
            .scope_index
            .enclosing_type(byte_of(source, "function G"))
            .unwrap();
        assert_eq!(doc.symbols.by_id(id).unwrap().name, "B");
    }

    #[test]
    fn local_redeclaration_returns_latest_before_byte() {
        let source = "function F() { var x : int; x = 1; var x : float; x = 2.0; }\n";
        let doc = parse_document(source).unwrap();
        let callable = doc
            .scope_index
            .enclosing_callable(byte_of(source, "x = 1"))
            .unwrap();

        let before_second = byte_of(source, "x = 1");
        let after_second = byte_of(source, "x = 2.0");

        let id_first = doc
            .scope_index
            .local_or_param_at(callable, "x", before_second)
            .unwrap();
        let id_second = doc
            .scope_index
            .local_or_param_at(callable, "x", after_second)
            .unwrap();

        assert_ne!(
            id_first, id_second,
            "shadow should resolve to a different SymbolId"
        );
        let first = doc.symbols.by_id(id_first).unwrap();
        let second = doc.symbols.by_id(id_second).unwrap();
        assert_eq!(first.type_annotation.as_deref(), Some("int"));
        assert_eq!(second.type_annotation.as_deref(), Some("float"));
    }

    #[test]
    fn parameter_is_visible_throughout_body() {
        let source = "function F(p : int) { p = 1; }\n";
        let doc = parse_document(source).unwrap();
        let callable = doc
            .scope_index
            .enclosing_callable(byte_of(source, "p = 1"))
            .unwrap();
        let id = doc
            .scope_index
            .local_or_param_at(callable, "p", byte_of(source, "p = 1"))
            .unwrap();
        assert_eq!(doc.symbols.by_id(id).unwrap().kind, SymbolKind::Parameter);
    }

    #[test]
    fn direct_member_skips_locals() {
        let source = "class C { function M() { var M : int; } }\n";
        let doc = parse_document(source).unwrap();
        let class_id = doc.scope_index.type_like_container("C").unwrap();
        let method_id = doc.scope_index.direct_member(class_id, "M").unwrap();
        assert_eq!(
            doc.symbols.by_id(method_id).unwrap().kind,
            SymbolKind::Method
        );
    }

    #[test]
    fn top_level_first_wins_matches_children_of() {
        let source = "function F() {} function F() {}\n";
        let doc = parse_document(source).unwrap();
        let id_index = doc.scope_index.top_level("F").unwrap();
        let id_legacy = doc
            .symbols
            .children_of(None)
            .find(|s| s.name == "F")
            .map(|s| s.id)
            .unwrap();
        assert_eq!(id_index, id_legacy);
    }

    #[test]
    fn type_like_container_returns_class_struct_state() {
        struct Case {
            label: &'static str,
            source: &'static str,
            name: &'static str,
            expected: SymbolKind,
        }
        let cases = [
            Case {
                label: "class",
                source: "class A {}\n",
                name: "A",
                expected: SymbolKind::Class,
            },
            Case {
                label: "struct",
                source: "struct S {}\n",
                name: "S",
                expected: SymbolKind::Struct,
            },
            Case {
                label: "state",
                source: "class Owner {} state St in Owner {}\n",
                name: "St",
                expected: SymbolKind::State,
            },
        ];
        for c in cases {
            let doc = parse_document(c.source).unwrap();
            let id = doc
                .scope_index
                .type_like_container(c.name)
                .unwrap_or_else(|| panic!("case {}: container missing", c.label));
            assert_eq!(
                doc.symbols.by_id(id).unwrap().kind,
                c.expected,
                "case {}",
                c.label
            );
        }
    }

    #[test]
    fn enclosing_returns_none_outside_any_body() {
        let source = "class A {}\n";
        let doc = parse_document(source).unwrap();
        assert!(doc.scope_index.enclosing_callable(0).is_none());
        assert!(doc.scope_index.enclosing_callable(source.len()).is_none());
    }
}
