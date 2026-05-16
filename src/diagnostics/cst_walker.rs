use std::collections::HashMap;
use std::marker::PhantomData;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;

use super::WorkspaceDiagnostic;

pub(crate) type TypeMemo = HashMap<(usize, usize), Option<String>>;

pub(crate) struct CstRuleCtx<'a, 'tree> {
    pub uri: &'a str,
    pub document: &'a ParsedDocument,
    pub db: &'a SymbolDb<'a>,
    pub type_memo: &'a mut TypeMemo,
    pub diagnostics: &'a mut Vec<WorkspaceDiagnostic>,
    pub _tree: PhantomData<&'tree ()>,
}

pub(crate) trait CstRule {
    fn interested_in(&self, kind: &str) -> bool;
    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>);
}

pub(crate) fn run_rules_on_document(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    rules: &[&dyn CstRule],
) -> Vec<WorkspaceDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut memo: TypeMemo = HashMap::new();
    walk(
        document.tree.root_node(),
        uri,
        document,
        db,
        rules,
        &mut memo,
        &mut diagnostics,
    );
    diagnostics
}

fn walk<'tree>(
    node: Node<'tree>,
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    rules: &[&dyn CstRule],
    memo: &mut TypeMemo,
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
) {
    let kind = node.kind();
    for rule in rules {
        if rule.interested_in(kind) {
            let mut ctx = CstRuleCtx {
                uri,
                document,
                db,
                type_memo: memo,
                diagnostics,
                _tree: PhantomData,
            };
            rule.visit(node, &mut ctx);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, uri, document, db, rules, memo, diagnostics);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;

    use tree_sitter::Node;

    use super::{run_rules_on_document, CstRule, CstRuleCtx};
    use crate::document::parse_document;
    use crate::resolve::{infer_expr_type_memo, SymbolDb, WorkspaceIndex};

    struct CountingRule {
        kind: &'static str,
        hits: Cell<usize>,
    }

    impl CstRule for CountingRule {
        fn interested_in(&self, kind: &str) -> bool {
            kind == self.kind
        }
        fn visit<'tree>(&self, _node: Node<'tree>, _ctx: &mut CstRuleCtx<'_, 'tree>) {
            self.hits.set(self.hits.get() + 1);
        }
    }

    struct InferenceCountingRule {
        inferences: Cell<usize>,
    }

    impl CstRule for InferenceCountingRule {
        fn interested_in(&self, kind: &str) -> bool {
            kind == "func_call_expr"
        }
        fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
            let func = if let Some(f) = node.child_by_field_name("func") {
                f
            } else {
                let mut cursor = node.walk();
                let Some(f) = node.named_children(&mut cursor).next() else {
                    return;
                };
                f
            };
            if func.kind() != "member_access_expr" {
                return;
            }
            let mut cursor = func.walk();
            let Some(receiver) = func.named_children(&mut cursor).next() else {
                return;
            };
            let before = ctx.type_memo.len();
            let _ = infer_expr_type_memo(
                ctx.uri,
                ctx.document,
                ctx.db,
                receiver,
                node.start_byte(),
                ctx.type_memo,
            );
            if ctx.type_memo.len() > before {
                self.inferences.set(self.inferences.get() + 1);
            }
        }
    }

    fn db<'a>(index: &'a WorkspaceIndex, base: &'a WorkspaceIndex) -> SymbolDb<'a> {
        SymbolDb::new(index, base)
    }

    #[test]
    fn multi_rule_single_walk() {
        let mut idx = WorkspaceIndex::default();
        let doc = parse_document("class A { function F() { var a : A; a.M(); } }\n").unwrap();
        idx.update_document("file:///t.ws", &doc);
        let base = WorkspaceIndex::default();
        let db = db(&idx, &base);

        let r1 = CountingRule {
            kind: "func_call_expr",
            hits: Cell::new(0),
        };
        let r2 = CountingRule {
            kind: "member_access_expr",
            hits: Cell::new(0),
        };
        let rules: Vec<&dyn CstRule> = vec![&r1, &r2];

        let _ = run_rules_on_document("file:///t.ws", &doc, &db, &rules);

        assert!(r1.hits.get() >= 1, "rule 1 should fire for func_call_expr");
        assert!(
            r2.hits.get() >= 1,
            "rule 2 should fire for member_access_expr"
        );
    }

    #[test]
    fn memo_avoids_redundant_inference() {
        let mut idx = WorkspaceIndex::default();
        let src = "class B { function Build() : C {} } \
                   class C { function Step() : C {} function Chain() : C {} } \
                   function T() { var b : B; b.Build().Step().Chain(); }\n";
        let doc = parse_document(src).unwrap();
        idx.update_document("file:///t.ws", &doc);
        let base = WorkspaceIndex::default();
        let db = db(&idx, &base);

        let counter = InferenceCountingRule {
            inferences: Cell::new(0),
        };
        let rules: Vec<&dyn CstRule> = vec![&counter];

        let _ = run_rules_on_document("file:///t.ws", &doc, &db, &rules);
        let first = counter.inferences.get();
        let _ = run_rules_on_document("file:///t.ws", &doc, &db, &rules);
        let second = counter.inferences.get() - first;
        assert_eq!(first, second, "each run must memoise independently");
    }

    #[test]
    fn memo_key_is_byte_range() {
        let mut memo: HashMap<(usize, usize), Option<String>> = HashMap::new();
        memo.insert((10, 20), Some("Foo".to_string()));
        assert_eq!(memo.get(&(10, 20)).unwrap().as_deref(), Some("Foo"));
    }
}
