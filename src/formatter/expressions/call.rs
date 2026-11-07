use std::iter;

use tree_sitter::Node;

use crate::cst::nav::first_child_kind;
use crate::cst::{fields, kinds};

use super::super::{Formatter, child_nodes, starts_below};
use super::LineEnd;

pub(super) struct CallParts<'t> {
    pub(super) func: Node<'t>,
    open: Node<'t>,
    args: Option<Node<'t>>,
    pub(super) close: Node<'t>,
}

impl<'t> CallParts<'t> {
    pub(super) fn of(call: Node<'t>) -> Option<Self> {
        if call.has_error() {
            return None;
        }
        Some(Self {
            func: call.child_by_field_name(fields::FUNC)?,
            open: first_child_kind(call, "(")?,
            args: call.child_by_field_name(fields::ARGS),
            close: first_child_kind(call, ")")?,
        })
    }

    fn arg_tokens(&self) -> Vec<Node<'t>> {
        let Some(args) = self.args else {
            return Vec::new();
        };
        child_nodes(args)
            .into_iter()
            .filter(|n| n.kind() != kinds::COMMENT)
            .collect()
    }

    fn hugs_lone_call_arg(&self) -> bool {
        match self.arg_tokens().as_slice() {
            [arg] => {
                arg.kind() == kinds::FUNC_CALL_EXPR
                    && CallParts::of(*arg).is_some_and(|inner| inner.args.is_some())
            }
            _ => false,
        }
    }

    fn authored_split(&self) -> bool {
        let tokens = self.arg_tokens();
        let line_heads = (0..tokens.len())
            .filter(|&index| opens_split_line(&tokens, index))
            .map(|index| tokens[index]);
        let anchors: Vec<Node> = iter::once(self.open)
            .chain(line_heads)
            .chain(iter::once(self.close))
            .collect();
        let breaks: Vec<bool> = anchors
            .windows(2)
            .map(|pair| starts_below(pair[0], pair[1]))
            .collect();
        breaks_request_split(&breaks)
    }
}

fn breaks_request_split(breaks: &[bool]) -> bool {
    let broken = breaks.iter().filter(|&&is_broken| is_broken).count();
    let intact = breaks.len() - broken;
    let close_on_own_line = breaks.last() == Some(&true);
    match (broken, intact) {
        (0, _) => false,
        (_, 0) => true,
        _ if broken == intact => !close_on_own_line,
        _ => broken < intact,
    }
}

// An omitted slot (two adjacent commas) must survive as a lone comma line, not vanish
fn opens_split_line(tokens: &[Node], index: usize) -> bool {
    let is_comma = |i: usize| tokens[i].kind() == ",";
    !is_comma(index) || index == 0 || is_comma(index - 1)
}

fn arg_line_end(arg: Node, next: Option<Node>) -> LineEnd {
    let comma = next.filter(|n| n.kind() == ",");
    LineEnd::after(comma.unwrap_or(arg), 0)
}

impl Formatter<'_> {
    pub(in crate::formatter) fn format_call(&mut self, call: Node) {
        if self.try_emit_fluent_chain(call) {
            return;
        }
        match CallParts::of(call) {
            Some(parts) if self.call_needs_split(call, &parts) => self.emit_split_call(&parts),
            _ => self.format_children(call),
        }
    }

    pub(super) fn format_fluent_call_args(&mut self, parts: &CallParts) {
        if self.call_needs_split(parts.open, parts) {
            self.emit_split_args(parts);
        } else {
            self.emit_inline_args(parts);
        }
    }

    fn call_needs_split(&self, measure_from: Node, parts: &CallParts) -> bool {
        if parts.args.is_none() {
            return false;
        }
        parts.authored_split()
            || self.has_line_comment_between(parts.open, parts.close)
            || (!parts.hugs_lone_call_arg() && self.overflows_line(measure_from))
    }

    fn emit_inline_args(&mut self, parts: &CallParts) {
        self.emit_verbatim(parts.open);
        if let Some(args) = parts.args {
            self.format_node(args);
        }
        self.emit_verbatim(parts.close);
    }

    fn emit_split_call(&mut self, parts: &CallParts) {
        self.with_line_end(LineEnd::after(parts.open, 0), |f| {
            f.format_node(parts.func);
        });
        self.emit_split_args(parts);
    }

    fn emit_split_args(&mut self, parts: &CallParts) {
        self.emit_verbatim(parts.open);
        self.level += 1;
        self.emit_arg_lines(parts);
        self.level -= 1;
        self.flush_comments_before(parts.close.start_byte());
        if !self.out.ends_with('\n') {
            self.nl();
        }
        self.emit_indent();
        self.emit_verbatim(parts.close);
    }

    fn emit_arg_lines(&mut self, parts: &CallParts) {
        let tokens = parts.arg_tokens();
        for (index, token) in tokens.iter().enumerate() {
            self.flush_comments_before(token.start_byte());
            if opens_split_line(&tokens, index) {
                self.break_to_indented_line();
            }
            if token.kind() == "," {
                self.emit(",");
                continue;
            }
            let line_end = arg_line_end(*token, tokens.get(index + 1).copied());
            self.with_line_end(line_end, |f| {
                f.format_operand(*token, kinds::FUNC_CALL_ARGS);
            });
        }
    }
}
