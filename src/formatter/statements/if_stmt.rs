use tree_sitter::Node;

use super::super::Formatter;
use super::BodyLayout;

impl<'a> Formatter<'a> {
    pub(in crate::formatter) fn format_if_stmt(&mut self, node: Node) {
        let layout = if self.if_chain_needs_block(node) {
            BodyLayout::ForceBlock
        } else {
            BodyLayout::Auto
        };
        self.format_if_stmt_emit(node, layout);
    }

    fn format_if_stmt_emit(&mut self, node: Node, layout: BodyLayout) {
        let cond = node.child_by_field_name("cond");
        let body = node.child_by_field_name("body");
        let else_body = node.child_by_field_name("else");

        if self.emit_split_keyword_cond("if (", cond) {
            self.emit_stmt_body(body, BodyLayout::ForceBlock);
        } else {
            self.emit_indent();
            self.emit("if (");
            if let Some(c) = cond {
                self.format_node(c);
            }
            self.emit(")");
            self.emit_stmt_body(body, layout);
        }

        if let Some(eb) = else_body {
            self.emit_indent();
            self.emit("else");
            self.emit_else_clause(eb, layout);
        }
    }

    fn emit_else_clause(&mut self, node: Node, layout: BodyLayout) {
        // An `else if` is another if-chain link, not a body slot; recurse to carry the layout.
        if node.kind() == "if_stmt" {
            self.emit(" ");
            self.suppress_next_indent = true;
            self.format_if_stmt_emit(node, layout);
            return;
        }
        self.emit_stmt_body(Some(node), layout);
    }

    fn if_chain_needs_block(&self, node: Node) -> bool {
        if let (Some(cond), Some(body)) = (
            node.child_by_field_name("cond"),
            node.child_by_field_name("body"),
        ) {
            if body.kind() != "func_block" {
                let indent = self.level * self.indent_unit.len();
                let line = indent + 4 + self.render_node(cond).len() + 2 + self.text(body).len();
                if line > self.line_limit {
                    return true;
                }
            }
        }
        self.else_chain_needs_block(node.child_by_field_name("else"))
    }

    fn else_chain_needs_block(&self, else_node: Option<Node>) -> bool {
        let Some(eb) = else_node else {
            return false;
        };
        match eb.kind() {
            "if_stmt" => {
                if let (Some(ec), Some(eb_body)) = (
                    eb.child_by_field_name("cond"),
                    eb.child_by_field_name("body"),
                ) {
                    if eb_body.kind() != "func_block" {
                        let indent = self.level * self.indent_unit.len();
                        let line =
                            indent + 9 + self.render_node(ec).len() + 2 + self.text(eb_body).len();
                        if line > self.line_limit {
                            return true;
                        }
                    }
                }
                self.else_chain_needs_block(eb.child_by_field_name("else"))
            }
            "func_block" => false,
            _ => {
                let indent = self.level * self.indent_unit.len();
                indent + 5 + self.text(eb).len() > self.line_limit
            }
        }
    }
}
