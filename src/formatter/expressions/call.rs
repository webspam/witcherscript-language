use tree_sitter::Node;

use crate::cst::nav::first_child_kind;
use crate::cst::{fields, kinds};

use super::super::{Formatter, child_nodes, starts_below};
use super::LineEnd;

struct CallParts<'t> {
    func: Node<'t>,
    open: Node<'t>,
    args: Node<'t>,
    close: Node<'t>,
}

impl<'t> CallParts<'t> {
    fn of(call: Node<'t>) -> Option<Self> {
        if call.has_error() {
            return None;
        }
        Some(Self {
            func: call.child_by_field_name(fields::FUNC)?,
            open: first_child_kind(call, "(")?,
            args: call.child_by_field_name(fields::ARGS)?,
            close: first_child_kind(call, ")")?,
        })
    }

    fn arg_tokens(&self) -> Vec<Node<'t>> {
        child_nodes(self.args)
            .into_iter()
            .filter(|n| n.kind() != kinds::COMMENT)
            .collect()
    }

    fn hugs_lone_call_arg(&self) -> bool {
        match self.arg_tokens().as_slice() {
            [arg] => arg.kind() == kinds::FUNC_CALL_EXPR && CallParts::of(*arg).is_some(),
            _ => false,
        }
    }

    fn authored_split(&self) -> bool {
        let tokens = self.arg_tokens();
        let Some(first) = tokens.first() else {
            return false;
        };
        if starts_below(self.open, *first) {
            return true;
        }
        let Some(comma) = tokens.iter().position(|t| t.kind() == ",") else {
            return false;
        };
        let first_arg_end = comma.checked_sub(1).map_or(self.open, |i| tokens[i]);
        tokens
            .get(comma + 1)
            .is_some_and(|second| starts_below(first_arg_end, *second))
    }
}

fn arg_line_end(arg: Node, following: &[Node]) -> LineEnd {
    let comma = following
        .iter()
        .find(|n| n.kind() != kinds::COMMENT)
        .filter(|n| n.kind() == ",");
    LineEnd::after(comma.copied().unwrap_or(arg), 0)
}

impl Formatter<'_> {
    pub(in crate::formatter) fn format_call(&mut self, call: Node) {
        match CallParts::of(call) {
            Some(parts) if self.call_needs_split(call, &parts) => self.emit_split_call(&parts),
            _ => self.format_children(call),
        }
    }

    fn call_needs_split(&self, call: Node, parts: &CallParts) -> bool {
        parts.authored_split()
            || self.has_line_comment_between(parts.open, parts.close)
            || (!parts.hugs_lone_call_arg() && self.overflows_line(call))
    }

    fn emit_split_call(&mut self, parts: &CallParts) {
        self.with_line_end(LineEnd::after(parts.open, 0), |f| {
            f.format_node(parts.func);
        });
        self.emit_verbatim(parts.open);
        self.level += 1;
        self.emit_arg_lines(parts.args);
        self.level -= 1;
        self.flush_comments_before(parts.close.start_byte());
        if !self.out.ends_with('\n') {
            self.nl();
        }
        self.emit_indent();
        self.emit_verbatim(parts.close);
    }

    // An omitted slot (two adjacent commas) must survive as a lone comma line, not vanish.
    fn emit_arg_lines(&mut self, args: Node) {
        let children = child_nodes(args);
        let mut pending_break = true;
        for (i, child) in children.iter().enumerate() {
            match child.kind() {
                // Flush, don't render: the cursor emits the comment once and consumes it, so the
                // end-of-statement sweep can't print it again.
                kinds::COMMENT => self.flush_comments_before(child.end_byte()),
                "," => {
                    if pending_break {
                        self.break_to_indented_line();
                    }
                    self.emit(",");
                    pending_break = true;
                }
                _ => {
                    self.flush_comments_before(child.start_byte());
                    if pending_break {
                        self.break_to_indented_line();
                        pending_break = false;
                    }
                    let line_end = arg_line_end(*child, &children[i + 1..]);
                    self.with_line_end(line_end, |f| {
                        f.format_operand(*child, kinds::FUNC_CALL_ARGS);
                    });
                }
            }
        }
    }
}
