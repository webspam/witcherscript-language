use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::resolve::{SymbolDb, enclosing_state_owner};

use super::{CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic, run_rules_on_document};

pub const KIND: &str = "parent_outside_state";

pub(crate) struct ParentOutsideStateRule;

impl CstRule for ParentOutsideStateRule {
    fn name(&self) -> &'static str {
        "parent_outside_state"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::PARENT_EXPR || kind == kinds::VIRTUAL_PARENT_EXPR
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_parent_keyword(node, ctx);
    }
}

pub fn collect_parent_outside_state_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = ParentOutsideStateRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(uri = %uri, count = diagnostics.len(), "emitted parent-outside-state diagnostics");
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for parent/virtual_parent outside a state"
    );
    result
}

fn check_parent_keyword<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    if enclosing_state_owner(ctx.document, ctx.db, node.start_byte()).is_some() {
        return;
    }

    let keyword = match node.kind() {
        kinds::VIRTUAL_PARENT_EXPR => "virtual_parent",
        _ => "parent",
    };
    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        node.start_byte(),
        node.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: KIND.to_string(),
        message: format!("'{keyword}' can only be used inside a state method."),
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
}

#[cfg(test)]
mod tests;
