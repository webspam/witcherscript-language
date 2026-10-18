use tree_sitter::Node;

use crate::cst::{fields, kinds};

use super::{CstRuleCtx, RelatedLocation, Severity, WorkspaceDiagnostic};

pub const KIND: &str = "short_state_name";

pub(super) fn short_state_name_diagnostic(
    ident: Node<'_>,
    name: &str,
    ctx: &CstRuleCtx<'_, '_>,
) -> Option<WorkspaceDiagnostic> {
    let annotation = ident
        .parent()
        .filter(|parent| parent.kind() == kinds::ANNOTATION)?;
    let mut states = ctx.db.states_named(name);
    if states.is_empty() {
        return None;
    }
    // safe: ident nodes are sliced from a UTF-8 String on char-aligned boundaries
    let annotation_name = annotation
        .child_by_field_name(fields::NAME)?
        .utf8_text(ctx.document.source.as_bytes())
        .ok()?;
    states.sort_by(|a, b| a.symbol.type_name().cmp(&b.symbol.type_name()));
    let suggestions = states
        .iter()
        .map(|state| format!("{annotation_name}({})", state.symbol.type_name()))
        .collect::<Vec<_>>()
        .join(", ");

    Some(WorkspaceDiagnostic {
        kind: KIND.to_string(),
        message: format!(
            "'{name}' is a state's short name, which annotations cannot target; \
             use the state's class name: {suggestions}"
        ),
        severity: Severity::Error,
        range: ctx.document.line_index.byte_range_to_range(
            &ctx.document.source,
            ident.start_byte(),
            ident.end_byte(),
        ),
        related: states
            .iter()
            .map(|state| RelatedLocation {
                uri: state.uri.clone(),
                range: state.symbol.selection_range,
                message: format!("state '{}' declared here", state.symbol.type_name()),
            })
            .collect(),
        data: None,
    })
}

#[cfg(test)]
mod tests;
