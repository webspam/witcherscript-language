use tree_sitter::Node;

use crate::cst::nav::first_child_kind;
use crate::cst::{fields, kinds};

use super::super::{Formatter, starts_below};
use super::LineEnd;

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
        .any(|(before, after)| starts_below(*before, *after))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OverflowWrap {
    Allowed,
    Denied,
}

impl Formatter<'_> {
    fn is_statement_value(&self, node: Node) -> bool {
        self.line_end
            .is_some_and(|line_end| line_end.statement_value_id == Some(node.id()))
    }

    pub(in crate::formatter) fn format_ternary(&mut self, node: Node) {
        let overflow_wrap = if self.is_statement_value(node) {
            OverflowWrap::Allowed
        } else {
            OverflowWrap::Denied
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
        let overflows = overflow_wrap == OverflowWrap::Allowed && self.overflows_line(node);
        if !parts.authored_break() && !overflows {
            return false;
        }
        self.with_line_end(LineEnd::after(parts.cond, 0), |f| {
            f.emit_ternary_operand(parts.cond, overflow_wrap);
        });
        self.level += 1;
        self.with_line_end(LineEnd::after(parts.conseq, 0), |f| {
            f.emit_ternary_link(parts.question, parts.conseq, overflow_wrap);
        });
        self.emit_ternary_link(parts.colon, parts.alt, overflow_wrap);
        self.level -= 1;
        true
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
