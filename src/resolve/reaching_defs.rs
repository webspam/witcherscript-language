//! Reaching-definitions for a single local variable over one function body.
//!
//! `WitcherScript` has only structured control flow (no goto, no labelled break/continue), so the
//! analysis is a structured recursive gen/kill walk rather than an explicit CFG with a worklist.
//! For each read of the target it reports which of the variable's definitions may reach it.

use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::nav::named_child_nodes;
use crate::cst::{fields, kinds};

use super::writes::{WriteSite, write_site_node};

/// A bitset over `LocalDefinition` indices; the 128 cap is arbitrary.
type Mask = u128;
const MAX_DEFS: usize = 128;

pub(super) struct LocalDefinition<'t> {
    /// The expression to substitute at a read, or `None` when there is no substitutable value.
    pub(super) value: Option<Node<'t>>,
    /// The statement that teardown deletes.
    pub(super) stmt: Option<Node<'t>>,
    pub(super) is_decl: bool,
    /// The definition takes effect once this node has been evaluated.
    owner: Node<'t>,
}

pub(super) struct ReachingDefs<'t> {
    /// Per read: its byte range and the index of the unique definition that reaches it, or `None`
    /// when zero or more than one definition reaches it.
    pub(super) per_read: Vec<(Range<usize>, Option<usize>)>,
    pub(super) all_defs: Vec<LocalDefinition<'t>>,
}

pub(super) fn reaching_defs<'t>(
    body: Node<'t>,
    decl: Node<'t>,
    names_len: usize,
    mutations: &[&WriteSite<'t>],
    reads: &[Range<usize>],
) -> ReachingDefs<'t> {
    let all_defs = build_defs(decl, names_len, mutations);
    if all_defs.len() > MAX_DEFS {
        let per_read = reads.iter().map(|r| (r.clone(), None)).collect();
        return ReachingDefs { per_read, all_defs };
    }

    let mut analyzer = Analyzer {
        defs: &all_defs,
        reads,
        out: vec![0; reads.len()],
    };
    analyzer.eval_block(&named_child_nodes(body), 0, Pass::Record);

    let per_read = reads
        .iter()
        .cloned()
        .zip(analyzer.out.iter().map(|&m| sole_def(m)))
        .collect();
    ReachingDefs { per_read, all_defs }
}

fn build_defs<'t>(
    decl: Node<'t>,
    names_len: usize,
    mutations: &[&WriteSite<'t>],
) -> Vec<LocalDefinition<'t>> {
    // A multi-name list shares one initialiser, so it is not the value of any single name.
    let decl_value = (names_len == 1)
        .then(|| decl.child_by_field_name(fields::INIT_VALUE))
        .flatten();
    let mut defs = vec![LocalDefinition {
        value: decl_value,
        stmt: None,
        is_decl: true,
        owner: decl,
    }];

    for site in mutations {
        let node = write_site_node(site);
        let value = match site {
            WriteSite::AssignTarget(_) => direct_assign_value(node),
            WriteSite::AssignBase(_) | WriteSite::OutArg(_) | WriteSite::ReceiverBase(_) => None,
        };
        defs.push(LocalDefinition {
            value,
            stmt: find_ancestor_of_kind(node, &[kinds::EXPR_STMT]),
            is_decl: false,
            owner: node,
        });
    }
    defs
}

fn direct_assign_value(target: Node<'_>) -> Option<Node<'_>> {
    let assign = find_ancestor_of_kind(target, &[kinds::ASSIGN_OP_EXPR])?;
    // A compound assignment folds in the prior value; only a plain `=` yields a substitutable RHS.
    let direct = assign
        .child_by_field_name(fields::OP)
        .is_some_and(|op| op.kind() == kinds::ASSIGN_OP_DIRECT);
    direct
        .then(|| assign.child_by_field_name(fields::RIGHT))
        .flatten()
}

fn sole_def(mask: Mask) -> Option<usize> {
    // Exactly one reaching definition; reads as that, not as a power-of-two test.
    #[allow(clippy::manual_is_power_of_two)]
    (mask.count_ones() == 1).then(|| mask.trailing_zeros() as usize)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Learn,
    Record,
}

struct Analyzer<'a, 't> {
    defs: &'a [LocalDefinition<'t>],
    reads: &'a [Range<usize>],
    out: Vec<Mask>,
}

#[derive(Clone, Copy)]
struct Flow {
    /// State at the fall-off-the-end exit, or `None` when control cannot reach it.
    normal: Option<Mask>,
    breaks: Option<Mask>,
    continues: Option<Mask>,
    returns: Option<Mask>,
}

impl Flow {
    fn pass(state: Mask) -> Self {
        Flow {
            normal: Some(state),
            breaks: None,
            continues: None,
            returns: None,
        }
    }
}

fn union(a: Option<Mask>, b: Option<Mask>) -> Option<Mask> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x | y),
        (x, None) | (None, x) => x,
    }
}

impl<'t> Analyzer<'_, 't> {
    fn eval_block(&mut self, stmts: &[Node<'t>], state: Mask, pass: Pass) -> Flow {
        let mut flow = Flow::pass(state);
        for stmt in stmts {
            let Some(cur) = flow.normal else { break };
            let next = self.eval_stmt(*stmt, cur, pass);
            flow = Flow {
                normal: next.normal,
                breaks: union(flow.breaks, next.breaks),
                continues: union(flow.continues, next.continues),
                returns: union(flow.returns, next.returns),
            };
        }
        flow
    }

    fn eval_stmt(&mut self, stmt: Node<'t>, state: Mask, pass: Pass) -> Flow {
        match stmt.kind() {
            kinds::FUNC_BLOCK => self.eval_block(&named_child_nodes(stmt), state, pass),
            kinds::LOCAL_VAR_DECL_STMT | kinds::EXPR_STMT | kinds::DELETE_STMT => {
                Flow::pass(self.eval_leaf(stmt, state, pass))
            }
            kinds::RETURN_STMT => {
                let end = self.eval_leaf(stmt, state, pass);
                Flow {
                    normal: None,
                    breaks: None,
                    continues: None,
                    returns: Some(end),
                }
            }
            kinds::BREAK_STMT => Flow {
                normal: None,
                breaks: Some(state),
                continues: None,
                returns: None,
            },
            kinds::CONTINUE_STMT => Flow {
                normal: None,
                breaks: None,
                continues: Some(state),
                returns: None,
            },
            kinds::IF_STMT => self.eval_if(stmt, state, pass),
            kinds::WHILE_STMT => self.eval_while(stmt, state, pass),
            kinds::DO_WHILE_STMT => self.eval_do_while(stmt, state, pass),
            kinds::FOR_STMT => self.eval_for(stmt, state, pass),
            kinds::SWITCH_STMT => self.eval_switch(stmt, state, pass),
            kinds::NOP => Flow::pass(state),
            // An unmodelled statement: poison its reads so none resolves to one reaching definition.
            _ => {
                self.poison(stmt, pass);
                Flow::pass(state)
            }
        }
    }

    fn eval_leaf(&mut self, node: Node<'t>, state: Mask, pass: Pass) -> Mask {
        // A read here (an assignment's RHS) runs before this def lands, so record on the incoming state.
        self.record(node, state, pass);
        self.gen_in(node, state)
    }

    fn record(&mut self, region: Node, state: Mask, pass: Pass) {
        if pass == Pass::Learn {
            return;
        }
        let (start, end) = (region.start_byte(), region.end_byte());
        for (i, read) in self.reads.iter().enumerate() {
            if start <= read.start && read.end <= end {
                self.out[i] |= state;
            }
        }
    }

    fn poison(&mut self, region: Node, pass: Pass) {
        self.record(region, Mask::MAX, pass);
    }

    fn gen_in(&self, region: Node, state: Mask) -> Mask {
        // Only the latest definition in the region reaches afterward; earlier ones are overwritten.
        match (0..self.defs.len())
            .filter(|&i| within(self.defs[i].owner, region))
            .max_by_key(|&i| self.defs[i].owner.start_byte())
        {
            Some(i) => 1u128 << i,
            None => state,
        }
    }

    fn eval_if(&mut self, stmt: Node<'t>, state: Mask, pass: Pass) -> Flow {
        if let Some(cond) = stmt.child_by_field_name(fields::COND) {
            self.record(cond, state, pass);
        }
        let then_flow = match stmt.child_by_field_name(fields::BODY) {
            Some(body) => self.eval_stmt(body, state, pass),
            None => Flow::pass(state),
        };
        // An `else if` is just another `if_stmt` in the `else` field, handled by recursion.
        let else_flow = match stmt.child_by_field_name(fields::ELSE) {
            Some(other) => self.eval_stmt(other, state, pass),
            None => Flow::pass(state),
        };
        Flow {
            normal: union(then_flow.normal, else_flow.normal),
            breaks: union(then_flow.breaks, else_flow.breaks),
            continues: union(then_flow.continues, else_flow.continues),
            returns: union(then_flow.returns, else_flow.returns),
        }
    }

    fn eval_while(&mut self, stmt: Node<'t>, state: Mask, pass: Pass) -> Flow {
        let body = stmt.child_by_field_name(fields::BODY);
        // Pass 1 (no recording) learns the definitions the body carries back to the loop header.
        let body1 = self.eval_body(body, state, Pass::Learn);
        let header = union(Some(state), union(body1.normal, body1.continues)).unwrap_or(state);
        if let Some(cond) = stmt.child_by_field_name(fields::COND) {
            self.record(cond, header, pass);
        }
        let body2 = self.eval_body(body, header, pass);
        Flow {
            // The loop exits when the condition is false (header state) or via `break`.
            normal: union(Some(header), body2.breaks),
            breaks: None,
            continues: None,
            returns: body2.returns,
        }
    }

    fn eval_do_while(&mut self, stmt: Node<'t>, state: Mask, pass: Pass) -> Flow {
        let body = stmt.child_by_field_name(fields::BODY);
        let body1 = self.eval_body(body, state, Pass::Learn);
        // The body runs at least once, so the entry merges the first run (state) with the back-edge.
        let entry = union(Some(state), union(body1.normal, body1.continues)).unwrap_or(state);
        let body2 = self.eval_body(body, entry, pass);
        if let Some(cond) = stmt.child_by_field_name(fields::COND) {
            let at_cond = union(body2.normal, body2.continues).unwrap_or(entry);
            self.record(cond, at_cond, pass);
        }
        Flow {
            normal: union(body2.normal, body2.breaks),
            breaks: None,
            continues: None,
            returns: body2.returns,
        }
    }

    fn eval_for(&mut self, stmt: Node<'t>, state: Mask, pass: Pass) -> Flow {
        let body = stmt.child_by_field_name(fields::BODY);
        let mut entry = state;
        if let Some(init) = stmt.child_by_field_name(fields::INIT) {
            entry = self.eval_leaf(init, entry, pass);
        }
        let body1 = self.eval_body(body, entry, Pass::Learn);
        let after_body1 = union(body1.normal, body1.continues).unwrap_or(entry);
        let after_iter1 = match stmt.child_by_field_name(fields::ITER) {
            Some(iter) => self.gen_in(iter, after_body1),
            None => after_body1,
        };
        let header = union(Some(entry), Some(after_iter1)).unwrap_or(entry);
        if let Some(cond) = stmt.child_by_field_name(fields::COND) {
            self.record(cond, header, pass);
        }
        let body2 = self.eval_body(body, header, pass);
        if let Some(iter) = stmt.child_by_field_name(fields::ITER) {
            let after_body2 = union(body2.normal, body2.continues).unwrap_or(header);
            self.record(iter, after_body2, pass);
        }
        Flow {
            normal: union(Some(header), body2.breaks),
            breaks: None,
            continues: None,
            returns: body2.returns,
        }
    }

    fn eval_switch(&mut self, stmt: Node<'t>, state: Mask, pass: Pass) -> Flow {
        if let Some(cond) = stmt.child_by_field_name(fields::COND) {
            self.record(cond, state, pass);
        }
        let Some(block) = stmt.child_by_field_name(fields::BODY) else {
            return Flow::pass(state);
        };

        let entry = state;
        let mut cur: Option<Mask> = None;
        let mut breaks = None;
        let mut continues = None;
        let mut returns = None;
        let mut has_default = false;

        for section in named_child_nodes(block) {
            match section.kind() {
                kinds::SWITCH_CASE_LABEL => {
                    // Every label is reachable both by falling through and from the switch head.
                    cur = union(cur, Some(entry));
                    if let Some(value) = section.child_by_field_name(fields::VALUE) {
                        self.record(value, entry, pass);
                    }
                }
                kinds::SWITCH_DEFAULT_LABEL => {
                    has_default = true;
                    cur = union(cur, Some(entry));
                }
                _ => {
                    let flow = self.eval_stmt(section, cur.unwrap_or(entry), pass);
                    breaks = union(breaks, flow.breaks);
                    continues = union(continues, flow.continues);
                    returns = union(returns, flow.returns);
                    cur = flow.normal;
                }
            }
        }

        // Exit via `break`, by falling off the last section, or - with no `default` - by matching no
        // case at all.
        let mut normal = union(breaks, cur);
        if !has_default {
            normal = union(normal, Some(entry));
        }
        Flow {
            normal,
            breaks: None,
            continues,
            returns,
        }
    }

    fn eval_body(&mut self, body: Option<Node<'t>>, state: Mask, pass: Pass) -> Flow {
        match body {
            Some(node) => self.eval_stmt(node, state, pass),
            None => Flow::pass(state),
        }
    }
}

fn within(inner: Node, outer: Node) -> bool {
    inner.start_byte() >= outer.start_byte() && inner.end_byte() <= outer.end_byte()
}
