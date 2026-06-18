use tree_sitter::Node;

use crate::cst::grammar::member_access_member;
use crate::cst::kinds;
use crate::cst::nav::first_named_child;
use crate::resolve::infer_type_memo;
use crate::symbols::{AccessLevel, SymbolKind};

use super::{CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct StructTempMemberRule;

impl CstRule for StructTempMemberRule {
    fn name(&self) -> &'static str {
        "struct_property_on_temporary"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::MEMBER_ACCESS_EXPR
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_struct_temp_access(node, ctx);
    }
}

fn check_struct_temp_access<'tree>(
    node: Node<'tree>,
    ctx: &mut CstRuleCtx<'_, 'tree>,
) -> Option<()> {
    let receiver = first_named_child(node)?;
    // Only a function-call receiver is a temporary; a variable or field is an lvalue.
    if receiver.kind() != kinds::FUNC_CALL_EXPR {
        return None;
    }

    let member_ident = member_access_member(node)?;
    if member_ident.kind() != kinds::IDENT {
        return None;
    }
    let property = member_ident
        .utf8_text(ctx.document.source.as_bytes())
        .ok()?;

    ctx.telemetry.type_inferences += 1;
    let receiver_type = infer_type_memo(
        ctx.uri,
        ctx.document,
        ctx.db,
        receiver,
        member_ident.start_byte(),
        ctx.type_memo,
    )
    .to_db_string()?;

    let top = ctx.db.find_top_level(&receiver_type)?;
    if top.symbol.kind != SymbolKind::Struct {
        return None;
    }

    // An unknown name is `unknown_member`'s job; only flag real struct properties.
    ctx.db
        .find_member(&receiver_type, property, AccessLevel::Private)?;

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        member_ident.start_byte(),
        member_ident.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "struct_property_on_temporary".to_string(),
        message: format!(
            "Cannot access struct property '{property}' on a temporary '{receiver_type}'; assign the value to a local variable first."
        ),
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
    Some(())
}

#[cfg(test)]
mod tests;
