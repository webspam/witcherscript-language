use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::{infer_expr_type_memo, SymbolDb};
use crate::symbols::{AccessLevel, SymbolKind};

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct UnknownMethodRule;

impl CstRule for UnknownMethodRule {
    fn name(&self) -> &'static str {
        "unknown_method"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == "func_call_expr"
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        check_method_call(node, ctx);
    }
}

pub fn collect_unknown_method_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = UnknownMethodRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(
                uri = %uri,
                count = diagnostics.len(),
                "emitted unknown-method diagnostics"
            );
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for unknown method calls"
    );

    result
}

fn check_method_call<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(func) = node.child_by_field_name("func").or_else(|| {
        let mut cursor = node.walk();
        let child = node.named_children(&mut cursor).next();
        child
    }) else {
        return;
    };

    if func.kind() != "member_access_expr" {
        return;
    }

    let Some(receiver) = ({
        let mut cursor = func.walk();
        let child = func.named_children(&mut cursor).next();
        child
    }) else {
        return;
    };

    let Some(method_ident) = func.child_by_field_name("member").or_else(|| {
        let mut cursor = func.walk();
        let child = func.named_children(&mut cursor).nth(1);
        child
    }) else {
        return;
    };

    if method_ident.kind() != "ident" {
        return;
    }

    let Ok(method_name) = method_ident.utf8_text(ctx.document.source.as_bytes()) else {
        return;
    };

    ctx.telemetry.type_inferences += 1;
    let Some(receiver_type) = infer_expr_type_memo(
        ctx.uri,
        ctx.document,
        ctx.db,
        receiver,
        method_ident.start_byte(),
        ctx.type_memo,
    ) else {
        return;
    };

    ctx.telemetry.top_level_lookups += 1;
    let Some(top) = ctx.db.find_top_level(&receiver_type) else {
        return;
    };

    if !matches!(
        top.symbol.kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
    ) {
        return;
    }

    ctx.telemetry.member_lookups += 1;
    if ctx
        .db
        .find_member(&receiver_type, method_name, AccessLevel::Private)
        .is_some()
    {
        return;
    }

    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        method_ident.start_byte(),
        method_ident.end_byte(),
    );

    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "unknown_method".to_string(),
        message: format!("no method '{method_name}' on type '{receiver_type}'"),
        severity: Severity::Error,
        range,
        related: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::collect_unknown_method_diagnostics;
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
        collect_unknown_method_diagnostics(&doc_pairs, &db)
    }

    #[test]
    fn no_diagnostic_for_known_method() {
        let (idx, docs) = index_and_docs(&[(
            "file:///test.ws",
            "class Foo { function Bar() {} function Test() { var f : Foo; f.Bar(); } }\n",
        )]);

        let result = check(&idx, &docs);

        assert!(
            result.is_empty(),
            "known method should not produce diagnostic"
        );
    }

    #[test]
    fn no_diagnostic_inherited_method() {
        let (idx, docs) = index_and_docs(&[
            ("file:///a.ws", "class Base { function Inherited() {} }\n"),
            (
                "file:///b.ws",
                "class Child extends Base { function Test() { var c : Child; c.Inherited(); } }\n",
            ),
        ]);

        let result = check(&idx, &docs);

        assert!(
            result.is_empty(),
            "inherited method should not produce diagnostic"
        );
    }

    #[test]
    fn no_diagnostic_this_known_method() {
        let (idx, docs) = index_and_docs(&[(
            "file:///test.ws",
            "class Foo { function Bar() {} function Run() { this.Bar(); } }\n",
        )]);

        let result = check(&idx, &docs);

        assert!(
            result.is_empty(),
            "this.method() on known method should not produce diagnostic"
        );
    }

    #[test]
    fn no_diagnostic_unknown_receiver() {
        let (idx, docs) = index_and_docs(&[(
            "file:///test.ws",
            "function Test(x : Unknown) { x.Method(); }\n",
        )]);

        let result = check(&idx, &docs);

        assert!(
            result.is_empty(),
            "unknown receiver type should not produce diagnostic"
        );
    }

    #[test]
    fn no_diagnostic_primitive_receiver() {
        let (idx, docs) = index_and_docs(&[(
            "file:///test.ws",
            "function Test() { var n : int; n.Method(); }\n",
        )]);

        let result = check(&idx, &docs);

        assert!(
            result.is_empty(),
            "primitive receiver should not produce diagnostic"
        );
    }

    #[test]
    fn flags_unknown_method_on_known_type() {
        let (idx, docs) = index_and_docs(&[(
            "file:///test.ws",
            "class Foo { } function Test() { var f : Foo; f.Qux(); }\n",
        )]);

        let result = check(&idx, &docs);

        let diags = result
            .get("file:///test.ws")
            .expect("should have diagnostics");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "unknown_method");
        assert!(diags[0].message.contains("Qux"));
        assert!(diags[0].message.contains("Foo"));
    }

    #[test]
    fn flags_this_unknown_method() {
        let (idx, docs) = index_and_docs(&[(
            "file:///test.ws",
            "class Foo { function Run() { this.Nonexistent(); } }\n",
        )]);

        let result = check(&idx, &docs);

        let diags = result
            .get("file:///test.ws")
            .expect("should have diagnostics");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "unknown_method");
    }

    #[test]
    fn flags_struct_receiver() {
        let (idx, docs) = index_and_docs(&[(
            "file:///test.ws",
            "struct Vec3 { } function Test() { var v : Vec3; v.Normalize(); }\n",
        )]);

        let result = check(&idx, &docs);

        let diags = result
            .get("file:///test.ws")
            .expect("should have diagnostics");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "unknown_method");
    }

    #[test]
    fn no_false_positive_on_private_method() {
        let (idx, docs) = index_and_docs(&[(
            "file:///test.ws",
            "class Foo { private function Secret() {} function Test() { var f : Foo; f.Secret(); } }\n",
        )]);

        let result = check(&idx, &docs);

        assert!(
            result.is_empty(),
            "private method should not produce unknown_method diagnostic"
        );
    }

    #[test]
    fn flags_unknown_method_cross_file() {
        let (idx, docs) = index_and_docs(&[
            ("file:///types.ws", "class Widget { function Draw() {} }\n"),
            (
                "file:///use.ws",
                "function Test() { var w : Widget; w.Render(); }\n",
            ),
        ]);

        let result = check(&idx, &docs);

        let diags = result
            .get("file:///use.ws")
            .expect("use.ws should have diagnostics");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "unknown_method");
        assert!(diags[0].message.contains("Render"));
    }

    #[test]
    fn flags_chained_call_unknown() {
        let (idx, docs) = index_and_docs(&[
            (
                "file:///a.ws",
                "class Builder { function Build() : Result {} }\n",
            ),
            ("file:///b.ws", "class Result { }\n"),
            (
                "file:///c.ws",
                "function Test() { var b : Builder; b.Build().Missing(); }\n",
            ),
        ]);

        let result = check(&idx, &docs);

        let diags = result
            .get("file:///c.ws")
            .expect("c.ws should have diagnostics");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, "unknown_method");
        assert!(diags[0].message.contains("Missing"));
    }
}
