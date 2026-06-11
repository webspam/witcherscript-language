use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use crate::symbols::{AccessLevel, SymbolKind};

use super::{
    CstRule, CstRuleCtx, RelatedLocation, Severity, WorkspaceDiagnostic, run_rules_on_document,
};

pub const KIND: &str = "duplicate_inherited_field";

pub(crate) struct InheritedFieldRule;

impl CstRule for InheritedFieldRule {
    fn name(&self) -> &'static str {
        "inherited_field"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::MEMBER_VAR_DECL
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_inherited_field(node, ctx);
    }
}

pub fn collect_inherited_field_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = InheritedFieldRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(uri = %uri, count = diagnostics.len(), "emitted inherited-field diagnostics");
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned member field declarations"
    );
    result
}

// A field hidden behind a nearer same-named ancestor method is missed; one chain probe, accepted.
fn check_inherited_field<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    if node.child_by_field_name(fields::ANNOTATION).is_some() {
        return None;
    }
    let container = ctx
        .document
        .symbols
        .enclosing_symbol_at(node.start_byte(), &[SymbolKind::Class, SymbolKind::State])?;
    let parent = ctx.db.superclass_of(&container.name)?;

    let mut cursor = node.walk();
    let name_idents: Vec<Node<'tree>> = node
        .children_by_field_name(fields::NAMES, &mut cursor)
        .collect();
    for ident in name_idents {
        let name = ident.utf8_text(ctx.document.source.as_bytes()).ok()?;
        let Some(ancestor) = ctx.db.find_member(&parent, name, AccessLevel::Private) else {
            continue;
        };
        if ancestor.symbol.kind != SymbolKind::Field {
            continue;
        }
        let ancestor_class = ancestor.symbol.container_name.as_deref().unwrap_or(&parent);
        let range = ctx.document.line_index.byte_range_to_range(
            &ctx.document.source,
            ident.start_byte(),
            ident.end_byte(),
        );
        ctx.diagnostics.push(WorkspaceDiagnostic {
            kind: KIND.to_string(),
            message: format!(
                "Field '{name}' is already declared in ancestor class '{ancestor_class}'"
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
