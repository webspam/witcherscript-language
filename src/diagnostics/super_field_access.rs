use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::grammar::{call_callee, member_access_member};
use crate::cst::nav::first_named_child;
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct SuperFieldAccessRule;

impl CstRule for SuperFieldAccessRule {
    fn name(&self) -> &'static str {
        "super_field_access"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == "member_access_expr"
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_super_member(node, ctx);
    }
}

pub fn collect_super_field_access_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = SuperFieldAccessRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(
                uri = %uri,
                count = diagnostics.len(),
                "emitted super-field-access diagnostics"
            );
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for super.field accesses"
    );

    result
}

fn check_super_member<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    let receiver = first_named_child(node)?;
    if receiver.kind() != "super_expr" {
        return None;
    }
    let member_ident = member_access_member(node)?;
    if member_ident.kind() != "ident" {
        return None;
    }

    if is_callee_of_func_call(node) {
        return None;
    }

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        node.start_byte(),
        node.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "super_field_access".to_string(),
        message: "'super.' can only be used to call methods; access fields directly.".to_string(),
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
    Some(())
}

fn is_callee_of_func_call(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "func_call_expr" {
        return false;
    }
    call_callee(parent).map(|c| c.id()) == Some(node.id())
}

#[cfg(test)]
mod tests {
    use super::collect_super_field_access_diagnostics;
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
        collect_super_field_access_diagnostics(&doc_pairs, &db)
    }

    #[test]
    fn flags_super_field_read() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Base { var x : int; } \
             class Derived extends Base { function F() { var y : int; y = super.x; } }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").expect("expected diagnostic");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "super_field_access");
    }

    #[test]
    fn flags_super_field_assignment_target() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Base { var x : int; } \
             class Derived extends Base { function F() { super.x = 1; } }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").expect("expected diagnostic");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "super_field_access");
    }

    #[test]
    fn allows_super_method_call() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Base { function M() {} } \
             class Derived extends Base { function F() { super.M(); } }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
    }

    #[test]
    fn allows_this_field_access() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Base { var x : int; } \
             class Derived extends Base { function F() { var y : int; y = this.x; } }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
    }

    #[test]
    fn does_not_fire_inside_error_subtree() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Base { var x : int; } \
             class Derived extends Base { function F() { y = super. } }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(
            result.is_empty(),
            "expected no super_field_access diagnostics inside error subtree, got {result:?}"
        );
    }
}
