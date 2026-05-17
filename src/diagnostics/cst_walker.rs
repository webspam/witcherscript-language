use std::collections::HashMap;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;

use super::WorkspaceDiagnostic;

pub(crate) type TypeMemo = HashMap<(usize, usize), Option<String>>;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct RuleTelemetry {
    pub top_level_lookups: usize,
    pub member_lookups: usize,
    pub enum_variant_lookups: usize,
    pub type_inferences: usize,
    pub definition_resolutions: usize,
    pub branch_type_ref_us: u64,
    pub branch_member_access_us: u64,
    pub branch_member_default_us: u64,
    pub branch_func_bare_call_us: u64,
    pub branch_bare_us: u64,
    pub branch_type_ref_visits: u64,
    pub branch_member_access_visits: u64,
    pub branch_member_default_visits: u64,
    pub branch_func_bare_call_visits: u64,
    pub branch_bare_visits: u64,
    pub member_access_infer_us: u64,
    pub member_access_member_us: u64,
}

pub(crate) struct CstRuleCtx<'a, 'tree> {
    pub uri: &'a str,
    pub document: &'a ParsedDocument,
    pub db: &'a SymbolDb<'a>,
    pub type_memo: &'a mut TypeMemo,
    pub telemetry: &'a mut RuleTelemetry,
    pub diagnostics: &'a mut Vec<WorkspaceDiagnostic>,
    pub in_error_subtree: bool,
    pub _tree: PhantomData<&'tree ()>,
}

pub(crate) trait CstRule {
    fn name(&self) -> &'static str;
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
    let mut telemetry = RuleTelemetry::default();
    let mut rule_times: Vec<(Duration, usize)> = vec![(Duration::ZERO, 0); rules.len()];
    walk(
        document.tree.root_node(),
        uri,
        document,
        db,
        rules,
        &mut memo,
        &mut telemetry,
        &mut rule_times,
        &mut diagnostics,
        false,
    );
    for ((elapsed, visits), rule) in rule_times.iter().zip(rules.iter()) {
        tracing::debug!(
            rule = rule.name(),
            visits = visits,
            elapsed_us = elapsed.as_micros() as u64,
            "cst rule timing"
        );
    }
    tracing::debug!(
        top_level = telemetry.top_level_lookups,
        member = telemetry.member_lookups,
        enum_variant = telemetry.enum_variant_lookups,
        type_inference = telemetry.type_inferences,
        definition = telemetry.definition_resolutions,
        memo_size = memo.len(),
        "cst lookup counts"
    );
    tracing::debug!(
        type_ref_us = telemetry.branch_type_ref_us,
        type_ref_visits = telemetry.branch_type_ref_visits,
        member_access_us = telemetry.branch_member_access_us,
        member_access_visits = telemetry.branch_member_access_visits,
        member_access_infer_us = telemetry.member_access_infer_us,
        member_access_member_us = telemetry.member_access_member_us,
        member_default_us = telemetry.branch_member_default_us,
        member_default_visits = telemetry.branch_member_default_visits,
        func_bare_call_us = telemetry.branch_func_bare_call_us,
        func_bare_call_visits = telemetry.branch_func_bare_call_visits,
        bare_us = telemetry.branch_bare_us,
        bare_visits = telemetry.branch_bare_visits,
        "unknown_symbol branch timing"
    );
    diagnostics
}

#[allow(clippy::too_many_arguments)]
fn walk<'tree>(
    node: Node<'tree>,
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
    rules: &[&dyn CstRule],
    memo: &mut TypeMemo,
    telemetry: &mut RuleTelemetry,
    rule_times: &mut [(Duration, usize)],
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
    in_error_subtree: bool,
) {
    let kind = node.kind();
    let in_error_subtree = in_error_subtree
        || node.is_error()
        || node.is_missing()
        || kind == "incomplete_member_access_expr";
    for (i, rule) in rules.iter().enumerate() {
        if rule.interested_in(kind) {
            let start = Instant::now();
            let mut ctx = CstRuleCtx {
                uri,
                document,
                db,
                type_memo: memo,
                telemetry,
                diagnostics,
                in_error_subtree,
                _tree: PhantomData,
            };
            rule.visit(node, &mut ctx);
            rule_times[i].0 += start.elapsed();
            rule_times[i].1 += 1;
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            uri,
            document,
            db,
            rules,
            memo,
            telemetry,
            rule_times,
            diagnostics,
            in_error_subtree,
        );
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
