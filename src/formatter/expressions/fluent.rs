use std::iter;
use std::mem;

use tree_sitter::Node;

use crate::cst::grammar::member_access_member;
use crate::cst::nav::{first_child_kind, first_named_child};
use crate::cst::{fields, kinds};

use super::super::{Formatter, starts_below};
use super::LineEnd;
use super::call::CallParts;

const MIN_FLUENT_GROUPS: usize = 2;
const MIN_OVERFLOW_FLUENT_GROUPS: usize = 3;
const AUTHORED_BREAK_BOUNDARIES: usize = 2;

struct FluentLink<'t> {
    access: Node<'t>,
    receiver: Node<'t>,
    dot: Node<'t>,
    member: Node<'t>,
    call: Option<CallParts<'t>>,
}

impl<'t> FluentLink<'t> {
    fn of(node: Node<'t>) -> Option<Self> {
        let (access, call) = match node.kind() {
            kinds::FUNC_CALL_EXPR => {
                let call = CallParts::of(node)?;
                (call.func, Some(call))
            }
            _ => (node, None),
        };
        if access.kind() != kinds::MEMBER_ACCESS_EXPR {
            return None;
        }
        Some(Self {
            access,
            receiver: first_named_child(access)?,
            dot: first_child_kind(access, ".")?,
            member: member_access_member(access)?,
            call,
        })
    }
}

struct FluentGroup<'t> {
    links: Vec<FluentLink<'t>>,
    closing_paren: Node<'t>,
}

impl FluentGroup<'_> {
    fn starts_below(&self, previous_end: Node) -> bool {
        self.links
            .first()
            .is_some_and(|link| starts_below(previous_end, link.member))
    }
}

struct FluentChain<'t> {
    head: Node<'t>,
    groups: Vec<FluentGroup<'t>>,
}

impl<'t> FluentChain<'t> {
    fn of(root: Node<'t>) -> Option<Self> {
        if root.has_error() {
            return None;
        }
        let (base, links) = collect_links_in_source_order(root);
        let first_call = links.iter().position(|link| link.call.is_some())?;
        let head = match first_call {
            0 => base,
            _ => links[first_call - 1].access,
        };
        let groups = group_links(links.into_iter().skip(first_call));
        Some(Self { head, groups })
    }

    fn has_authored_break(&self) -> bool {
        let group_ends = iter::once(self.head).chain(self.groups.iter().map(|g| g.closing_paren));
        group_ends
            .zip(&self.groups)
            .take(AUTHORED_BREAK_BOUNDARIES)
            .any(|(previous_end, group)| group.starts_below(previous_end))
    }
}

fn collect_links_in_source_order(root: Node) -> (Node, Vec<FluentLink>) {
    let mut links = Vec::new();
    let mut current = root;
    while let Some(link) = FluentLink::of(current) {
        current = link.receiver;
        links.push(link);
    }
    links.reverse();
    (current, links)
}

fn group_links<'t>(links: impl Iterator<Item = FluentLink<'t>>) -> Vec<FluentGroup<'t>> {
    let mut groups = Vec::new();
    let mut pending = Vec::new();
    for link in links {
        let closing_paren = link.call.as_ref().map(|call| call.close);
        pending.push(link);
        if let Some(closing_paren) = closing_paren {
            groups.push(FluentGroup {
                links: mem::take(&mut pending),
                closing_paren,
            });
        }
    }
    groups
}

fn is_receiver_of_later_call(call: Node) -> bool {
    let mut receiver = call;
    while let Some(access) = receiver.parent()
        && access.kind() == kinds::MEMBER_ACCESS_EXPR
        && first_named_child(access) == Some(receiver)
    {
        let is_callee = access
            .parent()
            .is_some_and(|outer| outer.child_by_field_name(fields::FUNC) == Some(access));
        if is_callee {
            return true;
        }
        receiver = access;
    }
    false
}

impl Formatter<'_> {
    pub(super) fn try_emit_fluent_chain(&mut self, call: Node) -> bool {
        if is_receiver_of_later_call(call) {
            return false;
        }
        let Some(chain) = FluentChain::of(call) else {
            return false;
        };
        if !self.breaks_fluently(call, &chain) {
            return false;
        }
        self.emit_fluent_chain(&chain);
        true
    }

    fn breaks_fluently(&self, call: Node, chain: &FluentChain) -> bool {
        let group_count = chain.groups.len();
        if group_count < MIN_FLUENT_GROUPS || self.has_interior_comment(call) {
            return false;
        }
        chain.has_authored_break()
            || (group_count >= MIN_OVERFLOW_FLUENT_GROUPS && self.overflows_line(call))
    }

    fn emit_fluent_chain(&mut self, chain: &FluentChain) {
        self.with_line_end(LineEnd::after(chain.head, 0), |f| {
            f.format_node(chain.head);
        });
        self.level += 1;
        let last_index = chain.groups.len() - 1;
        for (index, group) in chain.groups.iter().enumerate() {
            self.break_to_indented_line();
            if index == last_index {
                self.emit_fluent_group(group);
            } else {
                self.with_line_end(LineEnd::after(group.closing_paren, 0), |f| {
                    f.emit_fluent_group(group);
                });
            }
        }
        self.level -= 1;
    }

    fn emit_fluent_group(&mut self, group: &FluentGroup) {
        for link in &group.links {
            self.emit_verbatim(link.dot);
            self.emit_verbatim(link.member);
            if let Some(call) = &link.call {
                self.format_fluent_call_args(call);
            }
        }
    }
}
