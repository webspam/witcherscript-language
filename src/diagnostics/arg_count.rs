use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::grammar::{call_callee, callee_ident, raw_arg_slots};
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::resolve::{SymbolDb, callee_params};
use crate::symbols::node_text;

use super::{CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic, run_rules_on_document};

pub(crate) struct ArgCountRule;

impl CstRule for ArgCountRule {
    fn name(&self) -> &'static str {
        "arg_count"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::FUNC_CALL_EXPR
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_arg_count(node, ctx);
    }
}

pub fn collect_arg_count_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = ArgCountRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(uri = %uri, count = diagnostics.len(), "emitted arg-count diagnostics");
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for argument-count mismatches"
    );
    result
}

fn check_arg_count<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    let params = callee_params(ctx.uri, ctx.document, ctx.db, node)?;
    let slots = raw_arg_slots(node);
    let callee = callee_ident(call_callee(node)?)?;
    let name = node_text(callee, &ctx.document.source);

    let message = if slots.len() > params.len() {
        format!(
            "'{name}' takes at most {} argument(s), but {} given",
            params.len(),
            slots.len()
        )
    } else {
        // A required param is unmet if its positional slot is empty (`f(a,,c)`) or absent (too few).
        let missing: Vec<&str> = params
            .iter()
            .enumerate()
            .filter(|(i, param)| !param.specifiers.is_optional() && !slot_filled(&slots, *i))
            .map(|(_, param)| param.name.as_str())
            .collect();
        if missing.is_empty() {
            return None;
        }
        format!(
            "'{name}' is missing required argument(s): {}",
            missing.join(", ")
        )
    };

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        callee.start_byte(),
        callee.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "arg_count_mismatch".to_string(),
        message,
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
    Some(())
}

fn slot_filled(slots: &[Option<Node>], index: usize) -> bool {
    matches!(slots.get(index), Some(Some(_)))
}

#[cfg(test)]
mod tests;
