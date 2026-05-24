use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::Add;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::{annotation_target_class, Definition, ObservationSet, SymbolDb};
use crate::symbols::SymbolKind;

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

impl Add for RuleTelemetry {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            top_level_lookups: self.top_level_lookups + other.top_level_lookups,
            member_lookups: self.member_lookups + other.member_lookups,
            enum_variant_lookups: self.enum_variant_lookups + other.enum_variant_lookups,
            type_inferences: self.type_inferences + other.type_inferences,
            definition_resolutions: self.definition_resolutions + other.definition_resolutions,
            branch_type_ref_us: self.branch_type_ref_us + other.branch_type_ref_us,
            branch_member_access_us: self.branch_member_access_us + other.branch_member_access_us,
            branch_member_default_us: self.branch_member_default_us
                + other.branch_member_default_us,
            branch_func_bare_call_us: self.branch_func_bare_call_us
                + other.branch_func_bare_call_us,
            branch_bare_us: self.branch_bare_us + other.branch_bare_us,
            branch_type_ref_visits: self.branch_type_ref_visits + other.branch_type_ref_visits,
            branch_member_access_visits: self.branch_member_access_visits
                + other.branch_member_access_visits,
            branch_member_default_visits: self.branch_member_default_visits
                + other.branch_member_default_visits,
            branch_func_bare_call_visits: self.branch_func_bare_call_visits
                + other.branch_func_bare_call_visits,
            branch_bare_visits: self.branch_bare_visits + other.branch_bare_visits,
            member_access_infer_us: self.member_access_infer_us + other.member_access_infer_us,
            member_access_member_us: self.member_access_member_us + other.member_access_member_us,
        }
    }
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

pub(crate) fn access_is_inside_declaring_class<'tree>(
    ident: Node<'tree>,
    def: &Definition,
    ctx: &CstRuleCtx<'_, 'tree>,
) -> bool {
    let Some(declarer) = def.symbol.container_name.as_deref() else {
        return false;
    };
    let byte = ident.start_byte();
    if let Some(enclosing) = ctx.document.symbols.enclosing_symbol_at(
        byte,
        &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
    ) {
        return enclosing.name == declarer;
    }
    let callable = ctx.document.symbols.enclosing_symbol_at(
        byte,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    );
    callable
        .and_then(annotation_target_class)
        .is_some_and(|target| target == declarer)
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
    diagnostics
}

fn is_error_subtree_root(node: Node) -> bool {
    node.is_error() || node.is_missing() || node.kind() == "incomplete_member_access_expr"
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
    let in_error_subtree = in_error_subtree || is_error_subtree_root(node);
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

#[derive(Default)]
pub(crate) struct ParallelRuleShard {
    pub memo: TypeMemo,
    pub telemetry: RuleTelemetry,
    pub diagnostics: Vec<WorkspaceDiagnostic>,
    pub observer: ObservationSet,
}

pub(crate) fn collect_nodes_with_error_subtree<'tree>(
    root: Node<'tree>,
    predicate: impl Fn(&str) -> bool,
) -> Vec<(Node<'tree>, bool)> {
    let mut out = Vec::new();
    collect_nodes_walk(root, false, &predicate, &mut out);
    out
}

fn collect_nodes_walk<'tree>(
    node: Node<'tree>,
    in_error_subtree: bool,
    predicate: &impl Fn(&str) -> bool,
    out: &mut Vec<(Node<'tree>, bool)>,
) {
    let kind = node.kind();
    let in_error_subtree = in_error_subtree || is_error_subtree_root(node);
    if predicate(kind) {
        out.push((node, in_error_subtree));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_nodes_walk(child, in_error_subtree, predicate, out);
    }
}

pub(crate) fn run_parallel_pass<'tree, F>(
    items: &[(Node<'tree>, bool)],
    db: &SymbolDb<'_>,
    visit: F,
) -> ParallelRuleShard
where
    F: Fn(
            Node<'tree>,
            bool,
            &SymbolDb<'_>,
            &mut TypeMemo,
            &mut RuleTelemetry,
            &mut Vec<WorkspaceDiagnostic>,
        ) + Sync,
{
    items
        .par_iter()
        .fold(
            || {
                (
                    ParallelRuleShard::default(),
                    Mutex::new(ObservationSet::default()),
                )
            },
            |(mut shard, observer), &(node, in_err)| {
                let local_db = db.with_observer(&observer);
                visit(
                    node,
                    in_err,
                    &local_db,
                    &mut shard.memo,
                    &mut shard.telemetry,
                    &mut shard.diagnostics,
                );
                (shard, observer)
            },
        )
        .map(|(mut shard, observer)| {
            shard.observer = observer.into_inner().expect("observer mutex poisoned");
            shard
        })
        .reduce(ParallelRuleShard::default, merge_shards)
}

fn merge_shards(mut a: ParallelRuleShard, b: ParallelRuleShard) -> ParallelRuleShard {
    a.telemetry = a.telemetry + b.telemetry;
    a.diagnostics.extend(b.diagnostics);
    a.observer.top_level.extend(b.observer.top_level);
    a.observer.members.extend(b.observer.members);
    a.observer.enum_variants.extend(b.observer.enum_variants);
    a
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;

    use tree_sitter::Node;

    use super::{run_rules_on_document, CstRule, CstRuleCtx};
    use crate::cst::grammar::call_callee;
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
