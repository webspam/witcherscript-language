use tree_sitter::Node;

use crate::cst::kinds;

use super::super::{
    ChainPart, Formatter, chain_has_break, chain_operator_leads, split_binary_chain,
};

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
        let leads = chain_operator_leads(&parts);
        self.format_node(parts[0].operand);
        self.level += 1;
        for window in parts.windows(2) {
            let op = window[0]
                .op
                .expect("non-final chain part carries its operator");
            let layout = ChainLinkLayout::of(window[0].break_after, leads);
            self.emit_chain_link(op, layout, &window[1]);
        }
        self.level -= 1;
        true
    }

    fn emit_chain_link(&mut self, op: &str, layout: ChainLinkLayout, next: &ChainPart) {
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
        self.format_node(next.operand);
    }
}

#[derive(Clone, Copy)]
enum ChainLinkLayout {
    Inline,
    LeadingBreak,
    TrailingBreak,
}

impl ChainLinkLayout {
    fn of(break_after: bool, operator_leads: bool) -> Self {
        match (break_after, operator_leads) {
            (false, _) => Self::Inline,
            (true, true) => Self::LeadingBreak,
            (true, false) => Self::TrailingBreak,
        }
    }
}
