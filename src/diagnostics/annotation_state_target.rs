use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;

use super::{
    CstRule, CstRuleCtx, RelatedLocation, Severity, WorkspaceDiagnostic, run_rules_on_document,
};

pub const KIND: &str = "annotation_targets_backing_class";

const MEMBER_INJECTION_ANNOTATIONS: [&str; 4] =
    ["@wrapMethod", "@replaceMethod", "@addMethod", "@addField"];

// A real class coincidentally named like `{Owner}State{Name}` would false-positive; accepted.
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
    let rule = AnnotationStateTargetRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(uri = %uri, count = diagnostics.len(), "emitted annotation-state-target diagnostics");
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned annotation targets"
    );
    result
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
    let backing = ctx.db.find_state_backing_class(arg_name)?;
    let state_name = backing.state_name().to_string();
    let state_decl = backing.as_class_definition();

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        arg.start_byte(),
        arg.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: KIND.to_string(),
        message: format!(
            "'{arg_name}' is a state's backing class name, which annotations cannot target; \
             use the short state name: {annotation_name}({state_name})"
        ),
        severity: Severity::Error,
        range,
        related: vec![RelatedLocation {
            uri: state_decl.uri.clone(),
            range: state_decl.symbol.selection_range,
            message: format!("state '{state_name}' declared here"),
        }],
        data: None,
    });
    Some(())
}

#[cfg(test)]
mod tests;
