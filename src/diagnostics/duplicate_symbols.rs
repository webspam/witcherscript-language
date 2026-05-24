use std::collections::HashMap;

use crate::resolve::WorkspaceIndex;
use crate::symbols::{Symbol, SymbolKind};

use super::{RelatedLocation, Severity, WorkspaceDiagnostic};

type Occurrences<'a> = HashMap<(&'a str, Option<&'a str>), Vec<(&'a str, &'a Symbol)>>;

pub fn collect_duplicate_symbol_diagnostics(
    index: &WorkspaceIndex,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let mut by_name: Occurrences = HashMap::new();
    for (uri, sym) in index.all_top_level() {
        if !is_declaration_kind(sym.kind) {
            continue;
        }
        // Annotated functions are @addMethod/@wrapMethod member injections, not fresh global names.
        if !sym.annotations.is_empty() {
            continue;
        }
        // States are scoped to their statemachine, so two `state Combat in X`/`in Y`
        // do not collide; only same-owner states share a name.
        let owner = if sym.kind == SymbolKind::State {
            sym.owner_class.as_deref()
        } else {
            None
        };
        by_name
            .entry((sym.name.as_str(), owner))
            .or_default()
            .push((uri, sym));
    }

    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();
    for ((name, _), occurrences) in by_name {
        if occurrences.len() < 2 {
            continue;
        }
        for (i, (uri, sym)) in occurrences.iter().enumerate() {
            let related = occurrences
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, (other_uri, other_sym))| RelatedLocation {
                    uri: other_uri.to_string(),
                    range: other_sym.selection_range,
                    message: format!("'{name}' also declared here"),
                })
                .collect();
            result
                .entry(uri.to_string())
                .or_default()
                .push(WorkspaceDiagnostic {
                    kind: "duplicate_symbol".to_string(),
                    message: "A class or function with that name already exists.".to_string(),
                    severity: Severity::Error,
                    range: sym.selection_range,
                    related,
                    data: None,
                });
        }
    }
    result
}

fn is_declaration_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::State
            | SymbolKind::Function
            | SymbolKind::Event
    )
}

#[cfg(test)]
mod tests;
