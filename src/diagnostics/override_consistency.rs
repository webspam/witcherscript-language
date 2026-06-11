use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use crate::symbols::SymbolKind;

use super::{
    CstRule, CstRuleCtx, RelatedLocation, Severity, WorkspaceDiagnostic, run_rules_on_document,
};

pub const KIND_WEAKER_ACCESS: &str = "override_weaker_access";

pub(crate) struct OverrideConsistencyRule;

impl CstRule for OverrideConsistencyRule {
    fn name(&self) -> &'static str {
        "override_consistency"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::FUNC_DECL
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_override(node, ctx);
    }
}

pub fn collect_override_consistency_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = OverrideConsistencyRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(uri = %uri, count = diagnostics.len(), "emitted override-consistency diagnostics");
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned method overrides"
    );
    result
}

fn check_override<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    if node.child_by_field_name(fields::ANNOTATION).is_some() {
        return None;
    }
    let container = ctx
        .document
        .symbols
        .enclosing_symbol_at(node.start_byte(), &[SymbolKind::Class, SymbolKind::State])?;
    let name_ident = node.child_by_field_name(fields::NAME)?;
    // safe: ident nodes are sliced from a UTF-8 String on char-aligned boundaries
    let name = name_ident.utf8_text(ctx.document.source.as_bytes()).ok()?;
    let parent = ctx.db.superclass_of(&container.name)?;
    let ancestor = ctx.db.find_class_body_member(&parent, name)?;
    if ancestor.symbol.kind != SymbolKind::Method {
        return None;
    }
    let own = ctx
        .document
        .symbols
        .enclosing_symbol_at(name_ident.start_byte(), &[SymbolKind::Method])?;

    if own.access < ancestor.symbol.access {
        let ancestor_class = ancestor.symbol.container_name.as_deref().unwrap_or(&parent);
        let range = ctx.document.line_index.byte_range_to_range(
            &ctx.document.source,
            name_ident.start_byte(),
            name_ident.end_byte(),
        );
        ctx.diagnostics.push(WorkspaceDiagnostic {
            kind: KIND_WEAKER_ACCESS.to_string(),
            message: format!(
                "Function '{name}' cannot have a weaker access modifier than in \
                 ancestor class '{ancestor_class}'"
            ),
            severity: Severity::Error,
            range,
            related: vec![RelatedLocation {
                uri: ancestor.uri.clone(),
                range: ancestor.symbol.selection_range,
                message: format!("'{name}' declared here"),
            }],
            data: None,
        });
    }
    Some(())
}

#[cfg(test)]
mod tests;
