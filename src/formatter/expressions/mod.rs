mod call;
mod chain;
mod ternary;

use tree_sitter::Node;

use super::Formatter;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::formatter) struct LineEnd {
    end_byte: usize,
    tail_len: usize,
    statement_value_id: Option<usize>,
}

impl LineEnd {
    pub(in crate::formatter) fn after(node: Node, tail_len: usize) -> Self {
        Self {
            end_byte: node.end_byte(),
            tail_len,
            statement_value_id: None,
        }
    }

    pub(in crate::formatter) fn with_statement_value(self, value: Option<Node>) -> Self {
        Self {
            statement_value_id: value.map(|v| v.id()),
            ..self
        }
    }
}

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

    pub(in crate::formatter) fn with_line_end(
        &mut self,
        line_end: LineEnd,
        emit: impl FnOnce(&mut Self),
    ) {
        let outer = self.line_end.replace(line_end);
        emit(self);
        self.line_end = outer;
    }

    fn overflows_line(&self, node: Node) -> bool {
        self.line_end.is_some_and(|line_end| {
            let rest_of_line = self.render_span_from(node, line_end.end_byte).len();
            self.current_line_len() + rest_of_line + line_end.tail_len > self.line_limit
        })
    }
}
