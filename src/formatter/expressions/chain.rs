use tree_sitter::Node;

use crate::cst::kinds;

use super::super::{ChainPart, Formatter, chain_has_break, split_binary_chain};
use super::LineEnd;

impl Formatter<'_> {
    pub(in crate::formatter) fn try_emit_broken_chain(
        &mut self,
        node: Node,
        parent_kind: &str,
    ) -> bool {
        let is_chain = node.kind() == kinds::BINARY_OP_EXPR;
        let is_operand_of_binary = parent_kind == kinds::BINARY_OP_EXPR;
        if !is_chain || is_operand_of_binary || self.has_interior_comment(node) {
            return false;
        }
        let parts = split_binary_chain(node);
        if !chain_has_break(&parts) {
            return false;
        }
        let placement = chain_operator_placement(&parts);
        self.format_chain_operand(&parts, 0, placement);
        self.level += 1;
        for (i, window) in parts.windows(2).enumerate() {
            let op = window[0]
                .op
                .expect("non-final chain part carries its operator");
            let layout = ChainLinkLayout::of(window[0].break_after, placement);
            self.emit_chain_link(op, layout);
            self.format_chain_operand(&parts, i + 1, placement);
        }
        self.level -= 1;
        true
    }

    fn format_chain_operand(
        &mut self,
        parts: &[ChainPart],
        index: usize,
        placement: OperatorPlacement,
    ) {
        let operand = parts[index].operand;
        match segment_line_end(&parts[index..], placement) {
            Some(line_end) => self.with_line_end(line_end, |f| f.format_node(operand)),
            None => self.format_node(operand),
        }
    }

    fn emit_chain_link(&mut self, op: &str, layout: ChainLinkLayout) {
        match layout {
            ChainLinkLayout::Inline => {
                self.emit(" ");
                self.emit(op);
                self.emit(" ");
            }
            ChainLinkLayout::LeadingBreak => {
                self.nl();
                self.emit_indent();
                self.emit(op);
                self.emit(" ");
            }
            ChainLinkLayout::TrailingBreak => {
                self.emit(" ");
                self.emit(op);
                self.nl();
                self.emit_indent();
            }
        }
    }
}

fn segment_line_end(parts: &[ChainPart], placement: OperatorPlacement) -> Option<LineEnd> {
    let last_on_line = parts.iter().find(|p| p.break_after)?;
    let trailing_op_len = match (placement, last_on_line.op) {
        (OperatorPlacement::Trailing, Some(op)) => " ".len() + op.len(),
        (OperatorPlacement::Leading, _) | (OperatorPlacement::Trailing, None) => 0,
    };
    Some(LineEnd::after(last_on_line.operand, trailing_op_len))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OperatorPlacement {
    Leading,
    Trailing,
}

// The first break sets the whole chain's operator placement; later splits follow it.
fn chain_operator_placement(parts: &[ChainPart]) -> OperatorPlacement {
    let leads = parts
        .iter()
        .find(|p| p.break_after)
        .is_some_and(|p| p.op_leads);
    if leads {
        OperatorPlacement::Leading
    } else {
        OperatorPlacement::Trailing
    }
}

#[derive(Clone, Copy)]
enum ChainLinkLayout {
    Inline,
    LeadingBreak,
    TrailingBreak,
}

impl ChainLinkLayout {
    fn of(break_after: bool, placement: OperatorPlacement) -> Self {
        match (break_after, placement) {
            (false, _) => Self::Inline,
            (true, OperatorPlacement::Leading) => Self::LeadingBreak,
            (true, OperatorPlacement::Trailing) => Self::TrailingBreak,
        }
    }
}
