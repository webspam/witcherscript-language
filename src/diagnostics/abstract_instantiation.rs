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
    if def.symbol.kind != SymbolKind::Class || !def.symbol.is_abstract {
        return None;
    }

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        class_ident.start_byte(),
        class_ident.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "abstract_instantiation".to_string(),
        message: format!("Cannot instantiate abstract class '{name}'."),
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
    Some(())
}

#[cfg(test)]
mod tests {
    use super::collect_abstract_instantiation_diagnostics;
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
        collect_abstract_instantiation_diagnostics(&doc_pairs, &db)
    }

    #[test]
    fn flags_new_on_abstract_class() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "abstract class Base {} \
             function F() { var b : Base; b = new Base in this; }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").expect("expected diagnostic");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "abstract_instantiation");
        assert!(diags[0].message.contains("Base"));
    }

    #[test]
    fn allows_new_on_concrete_class() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Concrete {} \
             function F() { var c : Concrete; c = new Concrete in this; }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
    }

    #[test]
    fn flags_across_files() {
        let (idx, docs) = index_and_docs(&[
            ("file:///a.ws", "abstract class Base {}\n"),
            (
                "file:///b.ws",
                "function F() { var b : Base; b = new Base in this; }\n",
            ),
        ]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///b.ws").expect("expected diagnostic");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "abstract_instantiation");
    }

    #[test]
    fn ignores_unknown_class() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "function F() { var x : Missing; x = new Missing in this; }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
    }
}
