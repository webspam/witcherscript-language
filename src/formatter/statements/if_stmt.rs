use tree_sitter::Node;

use super::super::{
    chain_fully_broken, split_binary_condition, Formatter, IfToggle, LayoutDirective,
};
use super::BodyLayout;

const IF_OPEN: usize = "if (".len();
const ELSE_IF_OPEN: usize = "else if (".len();
const ELSE_OPEN: usize = "else ".len();
const COND_CLOSE: usize = ") ".len();

impl<'a> Formatter<'a> {
    pub(in crate::formatter) fn format_if_stmt(&mut self, node: Node) {
        let layout = match self.layout_directive.take() {
            Some(LayoutDirective::IfExpand) => BodyLayout::ForceBlock,
            Some(LayoutDirective::IfCollapse) => BodyLayout::ForceInline,
            _ if self.if_chain_needs_block(node) => BodyLayout::ForceBlock,
            _ => BodyLayout::Auto,
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
                let line = indent
                    + IF_OPEN
                    + self.render_node(cond).len()
                    + COND_CLOSE
                    + self.text(body).len();
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
                        let line = indent
                            + ELSE_IF_OPEN
                            + self.render_node(ec).len()
                            + COND_CLOSE
                            + self.text(eb_body).len();
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
                indent + ELSE_OPEN + self.text(eb).len() > self.line_limit
            }
        }
    }

    pub(in crate::formatter) fn if_toggle(&self, if_node: Node) -> IfToggle {
        let bodies = chain_bodies(if_node);
        let any_block = bodies.iter().any(|b| b.kind() == "func_block");
        let can_expand = bodies.iter().any(|b| body_expandable(*b));
        let all_collapsible = bodies.iter().all(|b| self.body_collapsible(*b));
        // A comment anywhere in the chain can't survive being pulled onto one line.
        let no_comments = self.comments.is_empty();
        let can_collapse =
            all_collapsible && any_block && no_comments && self.if_chain_collapse_fits(if_node);
        IfToggle {
            can_collapse,
            can_expand,
        }
    }

    fn body_collapsible(&self, body: Node) -> bool {
        match body.kind() {
            "func_block" => block_single_stmt(body).is_some_and(body_single_line),
            "nop" => false,
            _ => body_single_line(body),
        }
    }

    // Collapse must stay format-stable: every line fits and no condition is hand-broken (else fmt re-expands).
    fn if_chain_collapse_fits(&self, if_node: Node) -> bool {
        let indent = self.level * self.indent_unit.len();
        let mut cur = Some(if_node);
        let mut first = true;
        while let Some(n) = cur {
            if n.kind() != "if_stmt" {
                return indent + ELSE_OPEN + self.inline_body_len(n) <= self.line_limit;
            }
            let cond = n.child_by_field_name("cond");
            if cond.is_some_and(|c| chain_fully_broken(&split_binary_condition(c, self.source))) {
                return false;
            }
            let cond_len = cond.map(|c| self.render_node(c).len()).unwrap_or(0);
            let stmt_len = n
                .child_by_field_name("body")
                .map(|b| self.inline_body_len(b))
                .unwrap_or(0);
            let prefix = if first { IF_OPEN } else { ELSE_IF_OPEN };
            if indent + prefix + cond_len + COND_CLOSE + stmt_len > self.line_limit {
                return false;
            }
            first = false;
            cur = n.child_by_field_name("else");
        }
        true
    }

    fn inline_body_len(&self, body: Node) -> usize {
        let effective = block_single_stmt(body).unwrap_or(body);
        self.render_node(effective).len()
    }
}

fn chain_bodies(if_node: Node) -> Vec<Node> {
    let mut bodies = Vec::new();
    let mut cur = Some(if_node);
    while let Some(n) = cur {
        if n.kind() == "if_stmt" {
            if let Some(b) = n.child_by_field_name("body") {
                bodies.push(b);
            }
            cur = n.child_by_field_name("else");
        } else {
            bodies.push(n);
            cur = None;
        }
    }
    bodies
}

fn body_expandable(body: Node) -> bool {
    !matches!(body.kind(), "func_block" | "nop")
}

// Leaf `_func_stmt`s (end in `;`, no nested body); unwrapping one can't change `else` binding.
fn is_simple_stmt(node: Node) -> bool {
    matches!(
        node.kind(),
        "local_var_decl_stmt"
            | "break_stmt"
            | "continue_stmt"
            | "return_stmt"
            | "delete_stmt"
            | "expr_stmt"
    )
}

fn body_single_line(node: Node) -> bool {
    node.start_position().row == node.end_position().row
}

/// The lone simple statement in a `func_block`, else `None` (zero/many/compound) - the unwrap gate.
pub(super) fn block_single_stmt(block: Node) -> Option<Node> {
    if block.kind() != "func_block" {
        return None;
    }
    let mut cursor = block.walk();
    let mut stmts = block
        .children(&mut cursor)
        .filter(|c| c.is_named() && c.kind() != "nop" && c.kind() != "comment");
    let first = stmts.next()?;
    if stmts.next().is_some() {
        return None;
    }
    is_simple_stmt(first).then_some(first)
}
