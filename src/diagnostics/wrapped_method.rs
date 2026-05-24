use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::grammar::call_callee;
use crate::cst::nav::first_child_kind;
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use crate::symbols::SymbolKind;

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct WrappedMethodRule;

impl CstRule for WrappedMethodRule {
    fn name(&self) -> &'static str {
        "wrapped_method"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == "func_decl"
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        let _ = check_func_decl(node, ctx);
    }
}

pub fn collect_wrapped_method_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = WrappedMethodRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(
                uri = %uri,
                count = diagnostics.len(),
                "emitted wrapped-method diagnostics"
            );
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for wrapped-method rule"
    );

    result
}

fn check_func_decl<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    let symbol = ctx.document.symbols.enclosing_symbol_at(
        node.start_byte(),
        &[SymbolKind::Function, SymbolKind::Method],
    )?;
    if !symbol.annotations.iter().any(|a| a.name == "wrapMethod") {
        return None;
    }

    let body = first_child_kind(node, "func_block")?;

    let mut calls: Vec<Node<'tree>> = Vec::new();
    collect_wrapped_method_calls(body, ctx.document.source.as_bytes(), &mut calls);

    if calls.is_empty() {
        let name_node = node.child_by_field_name("name")?;
        push(
            ctx,
            name_node,
            "missing_wrapped_method",
            format!(
                "@wrapMethod function '{}' must call wrappedMethod(...) exactly once",
                symbol.name
            ),
        );
        return Some(());
    }

    for extra in calls.iter().skip(1) {
        push(
            ctx,
            *extra,
            "duplicate_wrapped_method",
            "wrappedMethod can only be called once in an @wrapMethod body; only the first call is expanded by the compiler".to_string(),
        );
    }

    Some(())
}

fn collect_wrapped_method_calls<'tree>(
    node: Node<'tree>,
    source: &[u8],
    out: &mut Vec<Node<'tree>>,
) {
    if node.kind() == "func_call_expr" {
        if let Some(ident) = bare_call_ident(node) {
            if ident.utf8_text(source).ok() == Some("wrappedMethod") {
                out.push(ident);
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_wrapped_method_calls(child, source, out);
    }
}

fn bare_call_ident<'tree>(call: Node<'tree>) -> Option<Node<'tree>> {
    let func = call_callee(call)?;
    if func.kind() == "ident" {
        Some(func)
    } else {
        None
    }
}

fn push<'tree>(ctx: &mut CstRuleCtx<'_, 'tree>, anchor: Node<'tree>, kind: &str, message: String) {
    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        anchor.start_byte(),
        anchor.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: kind.to_string(),
        message,
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
}

#[cfg(test)]
mod tests;
