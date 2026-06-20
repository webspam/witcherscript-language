use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::grammar::{
    ArgSlot, arg_slots_with_gaps, call_callee, call_close_paren, callee_ident,
};
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::resolve::{SymbolDb, callee_params};
use crate::symbols::node_text;

use super::{CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic, collect_single_rule_diagnostics};

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
    collect_single_rule_diagnostics(&ArgCountRule, documents, db)
}

fn check_arg_count<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    let params = callee_params(ctx.uri, ctx.document, ctx.db, node)?;
    let slots = arg_slots_with_gaps(node);
    let callee = callee_ident(call_callee(node)?)?;
    let name = node_text(callee, &ctx.document.source);

    let (message, start_byte, end_byte) = if slots.len() > params.len() {
        let first_extra = &slots[params.len()];
        // `slots.len() > params.len() >= 0` guarantees at least one slot.
        let last = slots.last().expect("slots is non-empty");
        let message = format!(
            "'{name}' takes at most {} argument(s), but {} given",
            params.len(),
            slots.len()
        );
        (message, first_extra.start_byte(), last.end_byte())
    } else {
        let mut missing: Vec<&str> = Vec::new();
        let mut first_unmet = None;
        for (i, param) in params.iter().enumerate() {
            // A required param is unmet if its positional slot is empty (`f(a,,c)`) or absent (too few).
            if param.specifiers.is_optional() || slots.get(i).is_some_and(ArgSlot::is_filled) {
                continue;
            }
            first_unmet.get_or_insert(i);
            missing.push(param.name.as_str());
        }
        let first_unmet = first_unmet?;
        let message = format!(
            "'{name}' is missing required argument(s): {}",
            missing.join(", ")
        );
        let gap = match slots.get(first_unmet) {
            Some(slot) => slot.start_byte(),
            None => call_close_paren(node)?.start_byte(),
        };
        (message, gap, gap)
    };

    let range =
        ctx.document
            .line_index
            .byte_range_to_range(&ctx.document.source, start_byte, end_byte);
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

#[cfg(test)]
mod tests;
