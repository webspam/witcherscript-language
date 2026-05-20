use std::collections::HashMap;

use crate::resolve::WorkspaceIndex;
use crate::symbols::{Symbol, SymbolId, SymbolKind};

use super::{RelatedLocation, Severity, WorkspaceDiagnostic};

const EXEMPT_ANNOTATIONS: &[&str] = &["wrapMethod", "replaceMethod"];

pub fn collect_duplicate_local_diagnostics(
    index: &WorkspaceIndex,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, syms) in index.documents() {
        let mut by_func: HashMap<SymbolId, Vec<&Symbol>> = HashMap::new();
        for sym in syms {
            if !matches!(sym.kind, SymbolKind::Parameter | SymbolKind::Variable) {
                continue;
            }
            let Some(func_id) = enclosing_callable_id(sym, syms) else {
                continue;
            };
            let func = &syms[func_id.0];
            if is_exempt(func) {
                continue;
            }
            by_func.entry(func_id).or_default().push(sym);
        }

        for bindings in by_func.values() {
            let mut by_name: HashMap<&str, Vec<&Symbol>> = HashMap::new();
            for sym in bindings {
                by_name.entry(sym.name.as_str()).or_default().push(*sym);
            }
            for (name, group) in by_name {
                if group.len() < 2 {
                    continue;
                }
                for (i, sym) in group.iter().enumerate() {
                    let related = group
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, other)| RelatedLocation {
                            uri: uri.to_string(),
                            range: other.selection_range,
                            message: format!("'{name}' also declared here"),
                        })
                        .collect();
                    result
                        .entry(uri.to_string())
                        .or_default()
                        .push(WorkspaceDiagnostic {
                            kind: "duplicate_local".to_string(),
                            message: format!("'{name}' is already declared in this function"),
                            severity: Severity::Error,
                            range: sym.selection_range,
                            related,
                            data: None,
                        });
                }
            }
        }
    }

    result
}

fn enclosing_callable_id(sym: &Symbol, doc_symbols: &[Symbol]) -> Option<SymbolId> {
    let mut current = sym.container?;
    loop {
        let parent = doc_symbols.get(current.0)?;
        if matches!(
            parent.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Event
        ) {
            return Some(current);
        }
        current = parent.container?;
    }
}

fn is_exempt(func: &Symbol) -> bool {
    func.annotations
        .iter()
        .any(|a| EXEMPT_ANNOTATIONS.contains(&a.name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::collect_duplicate_local_diagnostics;
    use crate::document::parse_document;
    use crate::resolve::WorkspaceIndex;

    fn index(docs: &[(&str, &str)]) -> WorkspaceIndex {
        let mut idx = WorkspaceIndex::default();
        for (uri, src) in docs {
            let doc = parse_document(*src).expect("parse should succeed");
            idx.update_document(*uri, &doc);
        }
        idx
    }

    #[test]
    fn param_and_local_same_name() {
        let idx = index(&[("file:///a.ws", "function F(x : int) {\n  var x : int;\n}\n")]);

        let result = collect_duplicate_local_diagnostics(&idx);

        let a = result.get("file:///a.ws").expect("a.ws flagged");
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|d| d.kind == "duplicate_local"));
        assert!(a.iter().all(|d| d.related.len() == 1));
    }

    #[test]
    fn two_locals_same_name() {
        let idx = index(&[(
            "file:///a.ws",
            "function F() {\n  var x : int;\n  var x : int;\n}\n",
        )]);

        let result = collect_duplicate_local_diagnostics(&idx);

        let a = result.get("file:///a.ws").expect("a.ws flagged");
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn same_name_in_different_functions_independent() {
        let idx = index(&[(
            "file:///a.ws",
            "function F() {\n  var x : int;\n}\nfunction G() {\n  var x : int;\n}\n",
        )]);

        assert!(collect_duplicate_local_diagnostics(&idx).is_empty());
    }

    #[test]
    fn wrap_method_exempt_from_duplicate_local() {
        let idx = index(&[(
            "file:///a.ws",
            "@wrapMethod(CR4Player)\nfunction F(x : int) {\n  var x : int;\n}\n",
        )]);

        assert!(
            collect_duplicate_local_diagnostics(&idx).is_empty(),
            "@wrapMethod must suppress duplicate_local"
        );
    }
}
