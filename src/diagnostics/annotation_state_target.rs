use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;

use super::{
    CstRule, CstRuleCtx, RelatedLocation, Severity, WorkspaceDiagnostic,
    collect_single_rule_diagnostics,
};

pub const KIND: &str = "short_state_name";

const MEMBER_INJECTION_ANNOTATIONS: [&str; 4] =
    ["@wrapMethod", "@replaceMethod", "@addMethod", "@addField"];

pub(crate) struct AnnotationStateTargetRule;

impl CstRule for AnnotationStateTargetRule {
    fn name(&self) -> &'static str {
        "annotation_state_target"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::ANNOTATION
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_annotation_target(node, ctx);
    }
}

pub fn collect_annotation_state_target_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    collect_single_rule_diagnostics(&AnnotationStateTargetRule, documents, db)
}

fn check_annotation_target<'tree>(
    node: Node<'tree>,
    ctx: &mut CstRuleCtx<'_, 'tree>,
) -> Option<()> {
    let name_node = node.child_by_field_name(fields::NAME)?;
    // safe: ident nodes are sliced from a UTF-8 String on char-aligned boundaries
    let annotation_name = name_node.utf8_text(ctx.document.source.as_bytes()).ok()?;
    if !MEMBER_INJECTION_ANNOTATIONS.contains(&annotation_name) {
        return None;
    }

    let arg = node.child_by_field_name(fields::ARG)?;
    let arg_name = arg.utf8_text(ctx.document.source.as_bytes()).ok()?;
    if ctx.db.find_top_level(arg_name).is_some() {
        return None;
    }
    let mut states = ctx.db.states_named(arg_name);
    if states.is_empty() {
        return None;
    }
    states.sort_by(|a, b| a.symbol.type_name().cmp(&b.symbol.type_name()));
    let suggestions = states
        .iter()
        .map(|state| format!("{annotation_name}({})", state.symbol.type_name()))
        .collect::<Vec<_>>()
        .join(", ");

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        arg.start_byte(),
        arg.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: KIND.to_string(),
        message: format!(
            "'{arg_name}' is a state's short name, which annotations cannot target; \
             use the state's class name: {suggestions}"
        ),
        severity: Severity::Error,
        range,
        related: states
            .iter()
            .map(|state| RelatedLocation {
                uri: state.uri.clone(),
                range: state.symbol.selection_range,
                message: format!("state '{}' declared here", state.symbol.type_name()),
            })
            .collect(),
        data: None,
    });
    Some(())
}

#[cfg(test)]
mod tests;
