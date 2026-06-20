use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use crate::symbols::SymbolKind;

use super::{
    CstRule, CstRuleCtx, RelatedLocation, Severity, WorkspaceDiagnostic,
    collect_single_rule_diagnostics,
};

pub const KIND_NOT_STATEMACHINE: &str = "state_owner_not_statemachine";
pub const KIND_NOT_CLASS: &str = "state_owner_not_class";

pub(crate) struct StateOwnerRule;

impl CstRule for StateOwnerRule {
    fn name(&self) -> &'static str {
        "state_owner"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::STATE_DECL
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_state_owner(node, ctx);
    }
}

pub fn collect_state_owner_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    collect_single_rule_diagnostics(&StateOwnerRule, documents, db)
}

// The statemachine keyword is not inherited: only the literal owner's flag is checked.
fn check_state_owner<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    let owner_ident = node.child_by_field_name(fields::PARENT)?;
    // safe: ident nodes are sliced from a UTF-8 String on char-aligned boundaries
    let owner_name = owner_ident.utf8_text(ctx.document.source.as_bytes()).ok()?;
    let owner = ctx.db.find_top_level(owner_name)?;

    let is_class = owner.symbol.kind == SymbolKind::Class;
    if is_class && owner.symbol.specifiers.is_state_machine() {
        return None;
    }

    let (kind, severity, message) = if is_class {
        (
            KIND_NOT_STATEMACHINE,
            Severity::Warning,
            format!(
                "'{owner_name}' is not a state machine, so it cannot host a state. \
                 Did you forget the 'statemachine' keyword?"
            ),
        )
    } else {
        (
            KIND_NOT_CLASS,
            Severity::Error,
            format!(
                "'{owner_name}' is not a class; a state can only be declared in a state machine class."
            ),
        )
    };

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        owner_ident.start_byte(),
        owner_ident.end_byte(),
    );
    let diagnostic = WorkspaceDiagnostic {
        kind: kind.to_string(),
        message,
        severity,
        range,
        related: vec![RelatedLocation {
            uri: owner.uri.clone(),
            range: owner.symbol.selection_range,
            message: format!("'{owner_name}' declared here"),
        }],
        data: None,
    };
    ctx.diagnostics.push(diagnostic);
    Some(())
}

#[cfg(test)]
mod tests;
