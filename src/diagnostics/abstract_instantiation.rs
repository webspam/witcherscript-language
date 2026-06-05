use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use crate::symbols::SymbolKind;

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct AbstractInstantiationRule;

impl CstRule for AbstractInstantiationRule {
    fn name(&self) -> &'static str {
        "abstract_instantiation"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == "new_expr"
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_new_expr(node, ctx);
    }
}

pub fn collect_abstract_instantiation_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = AbstractInstantiationRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(
                uri = %uri,
                count = diagnostics.len(),
                "emitted abstract-instantiation diagnostics"
            );
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for abstract instantiations"
    );

    result
}

fn check_new_expr<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    let class_ident = node.child_by_field_name("class")?;
    if class_ident.kind() != "ident" {
        return None;
    }
    let name = class_ident.utf8_text(ctx.document.source.as_bytes()).ok()?;
    let def = ctx.db.find_top_level(name)?;
    let (kind, message) = if def.symbol.kind == SymbolKind::NativeType {
        (
            "native_instantiation",
            format!("Cannot instantiate native type '{name}'."),
        )
    } else if def.symbol.kind == SymbolKind::Class && def.symbol.is_abstract {
        (
            "abstract_instantiation",
            format!("Cannot instantiate abstract class '{name}'."),
        )
    } else {
        return None;
    };

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        class_ident.start_byte(),
        class_ident.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: kind.to_string(),
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
