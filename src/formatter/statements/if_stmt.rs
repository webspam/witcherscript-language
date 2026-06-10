use tree_sitter::Node;

use crate::cst::{fields, kinds};

use super::super::action::LayoutCtx;
use super::super::{Formatter, IfToggle};
use super::BodyLayout;

const IF_OPEN: usize = "if (".len();
const ELSE_IF_OPEN: usize = "else if (".len();
const ELSE_OPEN: usize = "else ".len();
const COND_CLOSE: usize = ") ".len();

impl Formatter<'_> {
    pub(in crate::formatter) fn format_if_stmt(&mut self, node: Node) {
        let layout = if self.if_chain_needs_block(node) {
            BodyLayout::ForceBlock
        } else {
            BodyLayout::Auto
        };
        self.format_if_stmt_emit(node, layout);
    }

    fn format_if_stmt_emit(&mut self, node: Node, layout: BodyLayout) {
        let cond = node.child_by_field_name(fields::COND);
        let body = node.child_by_field_name(fields::BODY);
        let else_body = node.child_by_field_name(fields::ELSE);

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
        if node.kind() == kinds::IF_STMT {
            self.emit(" ");
            self.suppress_next_indent = true;
            self.format_if_stmt_emit(node, layout);
            return;
        }
        self.emit_stmt_body(Some(node), layout);
    }

    fn if_chain_needs_block(&self, node: Node) -> bool {
        self.if_link_overflows(node, IF_OPEN)
            || self.else_chain_needs_block(node.child_by_field_name(fields::ELSE))
    }

    fn else_chain_needs_block(&self, else_node: Option<Node>) -> bool {
        let Some(eb) = else_node else {
            return false;
        };
        match eb.kind() {
            kinds::IF_STMT => {
                self.if_link_overflows(eb, ELSE_IF_OPEN)
                    || self.else_chain_needs_block(eb.child_by_field_name(fields::ELSE))
            }
            kinds::FUNC_BLOCK => false,
            _ => {
                let indent = self.level * self.indent_unit.len();
                indent + ELSE_OPEN + self.text(eb).len() > self.line_limit
            }
        }
    }

    fn if_link_overflows(&self, node: Node, open: usize) -> bool {
        let (Some(cond), Some(body)) = (
            node.child_by_field_name(fields::COND),
            node.child_by_field_name(fields::BODY),
        ) else {
            return false;
        };
        // Block bodies never overflow
        if body.kind() == kinds::FUNC_BLOCK {
            return false;
        }
        let indent = self.level * self.indent_unit.len();
        let line =
            indent + open + self.render_node(cond).len() + COND_CLOSE + self.text(body).len();
        line > self.line_limit
    }
}

impl LayoutCtx<'_> {
    pub(in crate::formatter) fn if_toggle(&self, if_node: Node) -> IfToggle {
        let bodies = chain_bodies(if_node);
        let any_block = bodies.iter().any(|b| b.kind() == kinds::FUNC_BLOCK);
        let can_expand = bodies.iter().any(|b| body_expandable(*b));
        let all_collapsible = bodies.iter().all(|b| body_collapsible(*b));
        // A comment anywhere in the chain can't survive being pulled onto one line.
        let no_comments = self.comments.is_empty();
        let can_collapse =
            all_collapsible && any_block && no_comments && self.if_chain_collapse_fits(if_node);
        IfToggle {
            can_collapse,
            can_expand,
        }
    }

    // A condition split across rows can't be joined verbatim, so such a chain isn't collapsible.
    fn if_chain_collapse_fits(&self, if_node: Node) -> bool {
        let indent = self.level * self.indent_width;
        let mut cur = Some(if_node);
        let mut first = true;
        while let Some(n) = cur {
            if n.kind() != kinds::IF_STMT {
                return indent + ELSE_OPEN + inline_body_byte_len(n) <= self.line_limit;
            }
            let cond = n.child_by_field_name(fields::COND);
            if cond.is_some_and(|c| c.start_position().row != c.end_position().row) {
                return false;
            }
            let cond_len = cond.map_or(0, span_len);
            let stmt_len = n
                .child_by_field_name(fields::BODY)
                .map_or(0, inline_body_byte_len);
            let prefix = if first { IF_OPEN } else { ELSE_IF_OPEN };
            if indent + prefix + cond_len + COND_CLOSE + stmt_len > self.line_limit {
                return false;
            }
            first = false;
            cur = n.child_by_field_name(fields::ELSE);
        }
        true
    }
}

fn body_collapsible(body: Node) -> bool {
    match body.kind() {
        kinds::FUNC_BLOCK => block_single_stmt(body).is_some_and(body_single_line),
        kinds::NOP => false,
        _ => body_single_line(body),
    }
}

fn inline_body_byte_len(body: Node) -> usize {
    let effective = block_single_stmt(body).unwrap_or(body);
    span_len(effective)
}

fn span_len(node: Node) -> usize {
    node.end_byte() - node.start_byte()
}

pub(in crate::formatter) fn chain_bodies(if_node: Node) -> Vec<Node> {
    let mut bodies = Vec::new();
    let mut cur = Some(if_node);
    while let Some(n) = cur {
        if n.kind() == kinds::IF_STMT {
            if let Some(b) = n.child_by_field_name(fields::BODY) {
                bodies.push(b);
            }
            cur = n.child_by_field_name(fields::ELSE);
        } else {
            bodies.push(n);
            cur = None;
        }
    }
    bodies
}

pub(in crate::formatter) fn body_expandable(body: Node) -> bool {
    !matches!(body.kind(), kinds::FUNC_BLOCK | kinds::NOP)
}

// Unwrapping one of these into a bare branch can't change `else` binding.
fn is_simple_stmt(node: Node) -> bool {
    matches!(
        node.kind(),
        kinds::LOCAL_VAR_DECL_STMT
            | kinds::BREAK_STMT
            | kinds::CONTINUE_STMT
            | kinds::RETURN_STMT
            | kinds::DELETE_STMT
            | kinds::EXPR_STMT
    )
}

fn body_single_line(node: Node) -> bool {
    node.start_position().row == node.end_position().row
}

/// The lone simple statement in a `func_block`, else `None` (zero, many, or compound).
pub(in crate::formatter) fn block_single_stmt(block: Node) -> Option<Node> {
    if block.kind() != kinds::FUNC_BLOCK {
        return None;
    }
    let mut cursor = block.walk();
    let mut stmts = block
        .children(&mut cursor)
        .filter(|c| c.is_named() && c.kind() != kinds::NOP && c.kind() != kinds::COMMENT);
    let first = stmts.next()?;
    if stmts.next().is_some() {
        return None;
    }
    is_simple_stmt(first).then_some(first)
}
