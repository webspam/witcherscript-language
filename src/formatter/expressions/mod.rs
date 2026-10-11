mod chain;
mod ternary;

use tree_sitter::Node;

use super::Formatter;

pub(in crate::formatter) use ternary::OverflowWrapTarget;

impl Formatter<'_> {
    pub(in crate::formatter) fn format_operand(&mut self, operand: Node, parent_kind: &str) {
        if !self.try_emit_broken_chain(operand, parent_kind) {
            self.format_node(operand);
        }
    }

    /// Start a new indented line. Skip if unnecessary (e.g. an EOL comment).
    pub(in crate::formatter) fn break_to_indented_line(&mut self) {
        if !self.out.ends_with('\n') {
            self.nl();
        }
        self.emit_indent();
    }
}
