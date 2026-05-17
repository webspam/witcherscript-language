use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use crate::symbols::SymbolKind;

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct WrappedMethodRule;

impl CstRule for WrappedMethodRule {
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
    let func = if let Some(f) = call.child_by_field_name("func") {
        f
    } else {
        let mut cursor = call.walk();
        let first = call.named_children(&mut cursor).next();
        first?
    };
    if func.kind() == "ident" {
        Some(func)
    } else {
        None
    }
}

fn first_child_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find(|c| c.kind() == kind);
    found
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
    });
}

#[cfg(test)]
mod tests {
    use super::collect_wrapped_method_diagnostics;
    use crate::document::{parse_document, ParsedDocument};
    use crate::resolve::{SymbolDb, WorkspaceIndex};

    fn index_and_docs(docs: &[(&str, &str)]) -> (WorkspaceIndex, Vec<(String, ParsedDocument)>) {
        let mut idx = WorkspaceIndex::default();
        let mut parsed = Vec::new();
        for (uri, src) in docs {
            let doc = parse_document(*src).expect("parse should succeed");
            idx.update_document(*uri, &doc);
            parsed.push((uri.to_string(), doc));
        }
        (idx, parsed)
    }

    fn check(
        idx: &WorkspaceIndex,
        docs: &[(String, ParsedDocument)],
    ) -> std::collections::HashMap<String, Vec<super::WorkspaceDiagnostic>> {
        let base = WorkspaceIndex::default();
        let db = SymbolDb::new(idx, &base);
        let doc_pairs: Vec<(&str, &ParsedDocument)> =
            docs.iter().map(|(uri, doc)| (uri.as_str(), doc)).collect();
        collect_wrapped_method_diagnostics(&doc_pairs, &db)
    }

    fn kinds(diags: &[super::WorkspaceDiagnostic]) -> Vec<&str> {
        diags.iter().map(|d| d.kind.as_str()).collect()
    }

    #[test]
    fn single_call_passes() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             @wrapMethod(Foo) function W() { wrappedMethod(); }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
    }

    #[test]
    fn missing_call_flagged() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             @wrapMethod(Foo) function W() {}\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["missing_wrapped_method"]);
        assert!(diags[0].message.contains("W"));
    }

    #[test]
    fn duplicate_calls_flagged() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             @wrapMethod(Foo) function W() { wrappedMethod(); wrappedMethod(); wrappedMethod(); }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(
            kinds(diags),
            vec!["duplicate_wrapped_method", "duplicate_wrapped_method"]
        );
    }

    #[test]
    fn unannotated_function_ignored() {
        let (idx, docs) =
            index_and_docs(&[("file:///t.ws", "function F() { wrappedMethod(); }\n")]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
    }

    #[test]
    fn add_method_annotation_ignored() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             @addMethod(Foo) function A() {}\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
    }

    #[test]
    fn wrap_method_with_call_inside_if_passes() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             @wrapMethod(Foo) function W() { if (true) { wrappedMethod(); } }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
    }

    #[test]
    fn member_access_does_not_count() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             @wrapMethod(Foo) function W() { this.wrappedMethod(); }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["missing_wrapped_method"]);
    }
}
