use tree_sitter::Node;

use crate::cst::nav::first_child_kind;
use crate::cst::{fields, kinds};

use super::super::Formatter;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::formatter) struct OverflowWrapTarget {
    pub value_id: usize,
    pub trailing_len: usize,
}

struct TernaryParts<'t> {
    cond: Node<'t>,
    question: Node<'t>,
    conseq: Node<'t>,
    colon: Node<'t>,
    alt: Node<'t>,
}

impl<'t> TernaryParts<'t> {
    fn of(node: Node<'t>) -> Option<Self> {
        if node.has_error() {
            return None;
        }
        Some(Self {
            cond: node.child_by_field_name(fields::COND)?,
            question: first_child_kind(node, "?")?,
            conseq: node.child_by_field_name(fields::CONSEQ)?,
            colon: first_child_kind(node, ":")?,
            alt: node.child_by_field_name(fields::ALT)?,
        })
    }

    fn authored_break(&self) -> bool {
        [
            (self.cond, self.question),
            (self.question, self.conseq),
            (self.conseq, self.colon),
            (self.colon, self.alt),
        ]
        .iter()
        .any(|(before, after)| after.start_position().row > before.end_position().row)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OverflowWrap {
    Allowed { trailing_len: usize },
    Denied,
}

impl Formatter<'_> {
    pub(in crate::formatter) fn format_ternary(&mut self, node: Node) {
        let overflow_wrap = match self.overflow_wrap_target {
            Some(target) if target.value_id == node.id() => OverflowWrap::Allowed {
                trailing_len: target.trailing_len,
            },
            _ => OverflowWrap::Denied,
        };
        self.format_ternary_with(node, overflow_wrap);
    }

    fn format_ternary_with(&mut self, node: Node, overflow_wrap: OverflowWrap) {
        if !self.try_emit_broken_ternary(node, overflow_wrap) {
            self.format_children(node);
        }
    }

    fn try_emit_broken_ternary(&mut self, node: Node, overflow_wrap: OverflowWrap) -> bool {
        let Some(parts) = TernaryParts::of(node) else {
            return false;
        };
        let overflows = match overflow_wrap {
            OverflowWrap::Allowed { trailing_len } => self.ternary_overflows(node, trailing_len),
            OverflowWrap::Denied => false,
        };
        if !parts.authored_break() && !overflows {
            return false;
        }
        self.emit_ternary_operand(parts.cond, overflow_wrap);
        self.level += 1;
        self.emit_ternary_link(parts.question, parts.conseq, overflow_wrap);
        self.emit_ternary_link(parts.colon, parts.alt, overflow_wrap);
        self.level -= 1;
        true
    }

    fn ternary_overflows(&self, node: Node, trailing_len: usize) -> bool {
        self.current_line_len() + self.render_node(node).len() + trailing_len > self.line_limit
    }

    fn emit_ternary_link(&mut self, token: Node, operand: Node, overflow_wrap: OverflowWrap) {
        self.flush_comments_before(token.start_byte());
        self.break_to_indented_line();
        self.emit(self.text(token));
        self.flush_comments_before(operand.start_byte());
        if self.out.ends_with('\n') {
            self.emit_indent();
        } else {
            self.emit(" ");
        }
        self.emit_ternary_operand(operand, overflow_wrap);
    }

    fn emit_ternary_operand(&mut self, operand: Node, overflow_wrap: OverflowWrap) {
        if operand.kind() == kinds::TERNARY_COND_EXPR {
            self.format_ternary_with(operand, overflow_wrap);
        } else {
            self.format_operand(operand, kinds::TERNARY_COND_EXPR);
        }
    }
}
