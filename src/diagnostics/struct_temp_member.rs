use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::grammar::member_access_member;
use crate::cst::kinds;
use crate::cst::nav::first_named_child;
use crate::document::ParsedDocument;
use crate::resolve::{SymbolDb, infer_type_memo};
use crate::symbols::{AccessLevel, SymbolKind};

use super::{CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic, run_rules_on_document};

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

pub fn collect_struct_temp_member_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = StructTempMemberRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(
                uri = %uri,
                count = diagnostics.len(),
                "emitted struct-temp-member diagnostics"
            );
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for struct property access on a temporary"
    );

    result
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
