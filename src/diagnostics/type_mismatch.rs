use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::{assignability, infer_type, Assignability, SymbolDb};
use crate::symbols::node_text;
use crate::types::Type;

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct TypeMismatchRule;

impl CstRule for TypeMismatchRule {
    fn name(&self) -> &'static str {
        "type_mismatch"
    }

    fn interested_in(&self, kind: &str) -> bool {
        matches!(kind, "local_var_decl_stmt" | "assign_op_expr")
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        match node.kind() {
            "local_var_decl_stmt" => check_var_decl(node, ctx),
            "assign_op_expr" => check_assignment(node, ctx),
            _ => {}
        }
    }
}

pub fn collect_type_mismatch_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = TypeMismatchRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(uri = %uri, count = diagnostics.len(), "emitted type-mismatch diagnostics");
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for type mismatches"
    );
    result
}

fn check_var_decl<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(value) = node.child_by_field_name("init_value") else {
        return;
    };
    let Some(var_type) = node.child_by_field_name("var_type") else {
        return;
    };
    let target = Type::from_annotation(&node_text(var_type, &ctx.document.source));
    let value_type = infer_type(ctx.uri, ctx.document, ctx.db, value, value.start_byte());
    report_if_incompatible(value, &value_type, &target, ctx);
}

fn check_assignment<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(op) = node.child_by_field_name("op") else {
        return;
    };
    if op.kind() != "assign_op_direct" {
        return;
    }
    let (Some(left), Some(right)) = (
        node.child_by_field_name("left"),
        node.child_by_field_name("right"),
    ) else {
        return;
    };
    let target = infer_type(ctx.uri, ctx.document, ctx.db, left, left.start_byte());
    let value_type = infer_type(ctx.uri, ctx.document, ctx.db, right, right.start_byte());
    report_if_incompatible(right, &value_type, &target, ctx);
}

fn report_if_incompatible<'tree>(
    value_node: Node<'tree>,
    value_type: &Type,
    target: &Type,
    ctx: &mut CstRuleCtx<'_, 'tree>,
) {
    if value_type.is_unknown() || target.is_unknown() {
        return;
    }
    if assignability(value_type, target, ctx.db) != Assignability::Incompatible {
        return;
    }
    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        value_node.start_byte(),
        value_node.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "type_mismatch".to_string(),
        message: format!("Cannot assign value of type '{value_type}' to '{target}'"),
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
}

#[cfg(test)]
mod tests;
