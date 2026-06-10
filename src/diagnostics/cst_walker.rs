use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::Add;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rayon::prelude::*;
use tree_sitter::Node;

use crate::cst::kinds;
use crate::cst::walk::{CstVisitor, Visit, walk};
use crate::document::ParsedDocument;
use crate::resolve::{Definition, ObservationSet, SymbolDb, annotation_target_class};
use crate::symbols::SymbolKind;

use super::WorkspaceDiagnostic;

pub(crate) type TypeMemo = HashMap<(usize, usize), crate::types::Type>;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct RuleTelemetry {
    pub top_level_lookups: usize,
    pub member_lookups: usize,
    pub enum_member_lookups: usize,
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
            enum_member_lookups: self.enum_member_lookups + other.enum_member_lookups,
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

pub(crate) fn declaring_class_of(def: &Definition) -> Option<&str> {
    def.symbol
        .container_name
        .as_deref()
        .or_else(|| annotation_target_class(&def.symbol))
}

pub(crate) fn access_is_inside_declaring_class<'tree>(
    ident: Node<'tree>,
    def: &Definition,
    ctx: &CstRuleCtx<'_, 'tree>,
) -> bool {
    let Some(declarer) = declaring_class_of(def) else {
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
    let mut rule_walk = RuleWalk {
        uri,
        document,
        db,
        rules,
        memo: HashMap::new(),
        telemetry: RuleTelemetry::default(),
        rule_times: vec![(Duration::ZERO, 0); rules.len()],
        diagnostics: Vec::new(),
        error_tracker: ErrorSubtreeTracker::default(),
    };
    walk(document.tree.root_node(), &mut rule_walk);
    for ((elapsed, visits), rule) in rule_walk.rule_times.iter().zip(rules.iter()) {
        tracing::trace!(
            rule = rule.name(),
            visits = visits,
            elapsed_us = elapsed.as_micros() as u64,
            "cst rule timing"
        );
    }
    tracing::trace!(
        top_level = rule_walk.telemetry.top_level_lookups,
        member = rule_walk.telemetry.member_lookups,
        enum_member = rule_walk.telemetry.enum_member_lookups,
        type_inference = rule_walk.telemetry.type_inferences,
        definition = rule_walk.telemetry.definition_resolutions,
        memo_size = rule_walk.memo.len(),
        "cst lookup counts"
    );
    rule_walk.diagnostics
}

fn is_error_subtree_root(node: Node) -> bool {
    node.is_error() || node.is_missing() || node.kind() == kinds::INCOMPLETE_MEMBER_ACCESS_EXPR
}

// Marks the outermost error-subtree root; relies on walk pairing every enter with one leave.
#[derive(Default)]
struct ErrorSubtreeTracker {
    depth: usize,
    // Depth of the outermost error-subtree root; nodes below it are in the error subtree.
    error_depth: Option<usize>,
}

impl ErrorSubtreeTracker {
    fn enter(&mut self, node: Node) -> bool {
        if self.error_depth.is_none() && is_error_subtree_root(node) {
            self.error_depth = Some(self.depth);
        }
        self.depth += 1;
        self.error_depth.is_some()
    }

    fn leave(&mut self) {
        self.depth -= 1;
        if self.error_depth == Some(self.depth) {
            self.error_depth = None;
        }
    }
}

struct RuleWalk<'a, 'db> {
    uri: &'a str,
    document: &'a ParsedDocument,
    db: &'a SymbolDb<'db>,
    rules: &'a [&'a dyn CstRule],
    memo: TypeMemo,
    telemetry: RuleTelemetry,
    rule_times: Vec<(Duration, usize)>,
    diagnostics: Vec<WorkspaceDiagnostic>,
    error_tracker: ErrorSubtreeTracker,
}

impl<'tree> CstVisitor<'tree> for RuleWalk<'_, '_> {
    fn enter(&mut self, node: Node<'tree>) -> Visit {
        let in_error_subtree = self.error_tracker.enter(node);
        let kind = node.kind();
        let rules = self.rules;
        for (i, rule) in rules.iter().enumerate() {
            if rule.interested_in(kind) {
                let start = Instant::now();
                let mut ctx = CstRuleCtx {
                    uri: self.uri,
                    document: self.document,
                    db: self.db,
                    type_memo: &mut self.memo,
                    telemetry: &mut self.telemetry,
                    diagnostics: &mut self.diagnostics,
                    in_error_subtree,
                    _tree: PhantomData,
                };
                rule.visit(node, &mut ctx);
                self.rule_times[i].0 += start.elapsed();
                self.rule_times[i].1 += 1;
            }
        }
        Visit::Children
    }

    fn leave(&mut self, _node: Node<'tree>) {
        self.error_tracker.leave();
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
    let mut collector = NodeCollector {
        predicate,
        out: Vec::new(),
        error_tracker: ErrorSubtreeTracker::default(),
    };
    walk(root, &mut collector);
    collector.out
}

struct NodeCollector<'tree, P> {
    predicate: P,
    out: Vec<(Node<'tree>, bool)>,
    error_tracker: ErrorSubtreeTracker,
}

impl<'tree, P: Fn(&str) -> bool> CstVisitor<'tree> for NodeCollector<'tree, P> {
    fn enter(&mut self, node: Node<'tree>) -> Visit {
        let in_error_subtree = self.error_tracker.enter(node);
        if (self.predicate)(node.kind()) {
            self.out.push((node, in_error_subtree));
        }
        Visit::Children
    }

    fn leave(&mut self, _node: Node<'tree>) {
        self.error_tracker.leave();
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
            shard.observer = observer.into_inner();
            shard
        })
        .reduce(ParallelRuleShard::default, merge_shards)
}

fn merge_shards(mut a: ParallelRuleShard, b: ParallelRuleShard) -> ParallelRuleShard {
    a.telemetry = a.telemetry + b.telemetry;
    a.diagnostics.extend(b.diagnostics);
    a.observer.top_level.extend(b.observer.top_level);
    a.observer.members.extend(b.observer.members);
    a.observer.enum_members.extend(b.observer.enum_members);
    a
}

#[cfg(test)]
mod tests;
