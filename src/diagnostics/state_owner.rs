use std::collections::HashMap;

use crate::resolve::WorkspaceIndex;
use crate::symbols::SymbolKind;

use super::{RelatedLocation, Severity, WorkspaceDiagnostic};

pub const KIND: &str = "state_owner_not_statemachine";

/// Flags `state X in Owner` where `Owner` is a known class without the
/// `statemachine` keyword. The keyword is not inherited: only the literal
/// owner's flag is checked, so a subclass of a state machine still warns.
pub fn collect_state_owner_diagnostics(
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, state) in workspace.all_top_level() {
        if state.kind != SymbolKind::State {
            continue;
        }
        let Some(owner_name) = state.owner_class.as_deref() else {
            continue;
        };
        // An owner we cannot resolve is the unknown_type rule's concern; we only
        // warn once we can positively prove the owner is a non-statemachine class.
        let Some(owner) = workspace
            .find_top_level(owner_name)
            .or_else(|| base.find_top_level(owner_name))
        else {
            continue;
        };
        // A non-class owner is an error reported elsewhere; statemachine owners are fine.
        if owner.symbol.kind != SymbolKind::Class || owner.symbol.is_state_machine {
            continue;
        }

        result
            .entry(uri.to_string())
            .or_default()
            .push(WorkspaceDiagnostic {
                kind: KIND.to_string(),
                message: format!(
                    "State '{}' targets '{owner_name}', which is not a state machine. \
                     Did you forget the 'statemachine' keyword on the class?",
                    state.name,
                ),
                severity: Severity::Warning,
                range: state.selection_range,
                related: vec![RelatedLocation {
                    uri: owner.uri.clone(),
                    range: owner.symbol.selection_range,
                    message: format!("'{owner_name}' declared here"),
                }],
                data: None,
            });
    }

    result
}

#[cfg(test)]
mod tests;
