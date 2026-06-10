use std::collections::HashMap;

use crate::resolve::WorkspaceIndex;
use crate::symbols::SymbolKind;

use super::{RelatedLocation, Severity, WorkspaceDiagnostic};

pub const KIND_NOT_STATEMACHINE: &str = "state_owner_not_statemachine";
pub const KIND_NOT_CLASS: &str = "state_owner_not_class";

/// Validates the owner of every `state X in Owner` in one scan:
/// - owner is a class without the `statemachine` keyword -> warning. The keyword
///   is not inherited, so only the literal owner's flag is checked.
/// - owner resolves to something that is not a class -> error.
///
/// An owner that does not resolve at all is left to the `unknown_type` rule.
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
        // An owner we cannot resolve is the unknown_type rule's concern.
        let Some(owner) = workspace
            .find_top_level(owner_name)
            .or_else(|| base.find_top_level(owner_name))
        else {
            continue;
        };

        let is_class = owner.symbol.kind == SymbolKind::Class;
        if is_class && owner.symbol.is_state_machine {
            continue;
        }

        let (kind, severity, message) = if is_class {
            (
                KIND_NOT_STATEMACHINE,
                Severity::Warning,
                format!(
                    "State '{}' targets '{owner_name}', which is not a state machine. \
                     Did you forget the 'statemachine' keyword on the class?",
                    state.name,
                ),
            )
        } else {
            (
                KIND_NOT_CLASS,
                Severity::Error,
                format!(
                    "State '{}' targets '{owner_name}', which is not a class. \
                     States can only be declared in a state machine class.",
                    state.name,
                ),
            )
        };

        result
            .entry(uri.to_string())
            .or_default()
            .push(WorkspaceDiagnostic {
                kind: kind.to_string(),
                message,
                severity,
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
