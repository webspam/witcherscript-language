use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::grammar::{call_callee, member_access_member};
use crate::cst::kinds;
use crate::cst::nav::first_named_child;
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;

use super::{CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic, collect_single_rule_diagnostics};

pub(crate) struct SuperFieldAccessRule;

impl CstRule for SuperFieldAccessRule {
    fn name(&self) -> &'static str {
        "super_field_access"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::MEMBER_ACCESS_EXPR
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
    collect_single_rule_diagnostics(&SuperFieldAccessRule, documents, db)
}

fn check_super_member<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    let receiver = first_named_child(node)?;
    if receiver.kind() != kinds::SUPER_EXPR {
        return None;
    }
    let member_ident = member_access_member(node)?;
    if member_ident.kind() != kinds::IDENT {
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
    if parent.kind() != kinds::FUNC_CALL_EXPR {
        return false;
    }
    call_callee(parent).map(|c| c.id()) == Some(node.id())
}

#[cfg(test)]
mod tests;
