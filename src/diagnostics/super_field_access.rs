use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::grammar::{call_callee, member_access_member};
use crate::cst::nav::first_named_child;
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct SuperFieldAccessRule;

impl CstRule for SuperFieldAccessRule {
    fn name(&self) -> &'static str {
        "super_field_access"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == "member_access_expr"
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_super_member(node, ctx);
    }
}

pub fn collect_super_field_access_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = SuperFieldAccessRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(
                uri = %uri,
                count = diagnostics.len(),
                "emitted super-field-access diagnostics"
            );
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for super.field accesses"
    );

    result
}

fn check_super_member<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    let receiver = first_named_child(node)?;
    if receiver.kind() != "super_expr" {
        return None;
    }
    let member_ident = member_access_member(node)?;
    if member_ident.kind() != "ident" {
        return None;
    }

    if is_callee_of_func_call(node) {
        return None;
    }

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        node.start_byte(),
        node.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "super_field_access".to_string(),
        message: "'super.' can only be used to call methods; access fields directly.".to_string(),
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
    Some(())
}

fn is_callee_of_func_call(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "func_call_expr" {
        return false;
    }
    call_callee(parent).map(|c| c.id()) == Some(node.id())
}

#[cfg(test)]
mod tests;
