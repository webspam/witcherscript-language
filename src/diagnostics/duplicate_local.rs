use std::collections::HashMap;

use crate::resolve::WorkspaceIndex;
use crate::symbols::{enclosing_callable_id, Symbol, SymbolId, SymbolKind};

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
            let Some(func_id) = enclosing_callable_id(syms, sym) else {
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

fn is_exempt(func: &Symbol) -> bool {
    func.annotations
        .iter()
        .any(|a| EXEMPT_ANNOTATIONS.contains(&a.name.as_str()))
}

#[cfg(test)]
mod tests;
