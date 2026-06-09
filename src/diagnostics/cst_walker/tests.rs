use std::cell::Cell;
use std::collections::HashMap;

use tree_sitter::Node;

use super::{CstRule, CstRuleCtx, run_rules_on_document};
use crate::cst::grammar::call_callee;
use crate::resolve::infer_type_memo;
use crate::test_support::TestDb;
use crate::types::Type;

struct CountingRule {
    kind: &'static str,
    hits: Cell<usize>,
}

impl CstRule for CountingRule {
    fn name(&self) -> &'static str {
        "counting"
    }
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
    fn name(&self) -> &'static str {
        "inference_counting"
    }
    fn interested_in(&self, kind: &str) -> bool {
        kind == "func_call_expr"
    }
    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        let Some(func) = call_callee(node) else {
            return;
        };
        if func.kind() != "member_access_expr" {
            return;
        }
        let mut cursor = func.walk();
        let Some(receiver) = func.named_children(&mut cursor).next() else {
            return;
        };
        let before = ctx.type_memo.len();
        let _ = infer_type_memo(
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

#[test]
fn multi_rule_single_walk() {
    let t = TestDb::new("class A { function F() { var a : A; a.M(); } }\n");

    let r1 = CountingRule {
        kind: "func_call_expr",
        hits: Cell::new(0),
    };
    let r2 = CountingRule {
        kind: "member_access_expr",
        hits: Cell::new(0),
    };
    let rules: Vec<&dyn CstRule> = vec![&r1, &r2];

    let _ = run_rules_on_document(t.primary_uri(), t.primary_doc(), &t.db(), &rules);

    assert!(r1.hits.get() >= 1, "rule 1 should fire for func_call_expr");
    assert!(
        r2.hits.get() >= 1,
        "rule 2 should fire for member_access_expr"
    );
}

#[test]
fn memo_avoids_redundant_inference() {
    let t = TestDb::new(
        "class B { function Build() : C {} } \
         class C { function Step() : C {} function Chain() : C {} } \
         function T() { var b : B; b.Build().Step().Chain(); }\n",
    );

    let counter = InferenceCountingRule {
        inferences: Cell::new(0),
    };
    let rules: Vec<&dyn CstRule> = vec![&counter];

    let _ = run_rules_on_document(t.primary_uri(), t.primary_doc(), &t.db(), &rules);
    let first = counter.inferences.get();
    let _ = run_rules_on_document(t.primary_uri(), t.primary_doc(), &t.db(), &rules);
    let second = counter.inferences.get() - first;
    assert_eq!(first, second, "each run must memoise independently");
}

#[test]
fn memo_key_is_byte_range() {
    let mut memo: HashMap<(usize, usize), Type> = HashMap::new();
    memo.insert((10, 20), Type::Named("Foo".to_string()));
    assert_eq!(memo.get(&(10, 20)), Some(&Type::Named("Foo".to_string())));
}
