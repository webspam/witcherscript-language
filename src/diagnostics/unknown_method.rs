use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::grammar::{call_callee, member_access_member};
use crate::cst::kinds;
use crate::cst::nav::first_named_child;
use crate::document::ParsedDocument;
use crate::resolve::{SymbolDb, infer_type_memo};
use crate::symbols::AccessLevel;

use super::{
    CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic, access_is_inside_declaring_class,
    collect_single_rule_diagnostics, declaring_class_of,
};

pub(crate) struct UnknownMethodRule;

impl CstRule for UnknownMethodRule {
    fn name(&self) -> &'static str {
        "unknown_method"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::FUNC_CALL_EXPR
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        check_method_call(node, ctx);
    }
}

pub fn collect_unknown_method_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    collect_single_rule_diagnostics(&UnknownMethodRule, documents, db)
}

fn check_method_call<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(func) = call_callee(node) else {
        return;
    };

    if func.kind() != kinds::MEMBER_ACCESS_EXPR {
        return;
    }

    let Some(receiver) = first_named_child(func) else {
        return;
    };

    let Some(method_ident) = member_access_member(func) else {
        return;
    };

    if method_ident.kind() != kinds::IDENT {
        return;
    }

    let Ok(method_name) = method_ident.utf8_text(ctx.document.source.as_bytes()) else {
        return;
    };

    ctx.telemetry.type_inferences += 1;
    let Some(receiver_type) = infer_type_memo(
        ctx.uri,
        ctx.document,
        ctx.db,
        receiver,
        method_ident.start_byte(),
        ctx.type_memo,
    )
    .to_db_string() else {
        return;
    };

    ctx.telemetry.top_level_lookups += 1;
    let Some(top) = ctx.db.find_top_level(&receiver_type) else {
        return;
    };

    if !top.symbol.kind.is_instantiable() {
        return;
    }

    ctx.telemetry.member_lookups += 1;
    if let Some(def) = ctx
        .db
        .find_member(&receiver_type, method_name, AccessLevel::Private)
    {
        if def.symbol.access == AccessLevel::Private
            && !access_is_inside_declaring_class(method_ident, &def, ctx)
        {
            let declarer = declaring_class_of(&def).unwrap_or("");
            let range = ctx.document.line_index.byte_range_to_range(
                &ctx.document.source,
                method_ident.start_byte(),
                method_ident.end_byte(),
            );
            ctx.diagnostics.push(WorkspaceDiagnostic {
                kind: "private_member_access".to_string(),
                message: format!(
                    "Private member '{method_name}' of class '{declarer}' is not accessible here."
                ),
                severity: Severity::Error,
                range,
                related: vec![],
                data: None,
            });
        }
        return;
    }

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        method_ident.start_byte(),
        method_ident.end_byte(),
    );

    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "unknown_method".to_string(),
        message: format!("No method '{method_name}' on type '{receiver_type}'"),
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
}

#[cfg(test)]
mod tests;
