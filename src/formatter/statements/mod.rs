mod switch;

use tree_sitter::Node;

use super::{
    chain_fully_broken, child_nodes, named_child_nodes, split_binary_condition,
    try_split_call_args, BoolPart, Formatter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::formatter) enum BodyLayout {
    Auto,
    ForceBlock,
}

impl<'a> Formatter<'a> {
    // ---- Function body ----

    pub(super) fn format_func_block(&mut self, node: Node) {
        self.format_func_block_inner(node, true);
    }

    fn format_func_block_inner(&mut self, node: Node, trailing_nl: bool) {
        let children = child_nodes(node);
        // Pair each named statement with whether its next sibling is ";" (can happen for
        // error-recovery nodes where the semicolon is not captured inside the error node).
        let stmts: Vec<(Node, bool)> = children
            .iter()
            .enumerate()
            .filter_map(|(i, child)| {
                if child.is_named() && child.kind() != "nop" && child.kind() != "comment" {
                    let trailing_semi = children
                        .get(i + 1)
                        .map(|n| n.kind() == ";" || n.kind() == "nop")
                        .unwrap_or(false);
                    Some((*child, trailing_semi))
                } else {
                    None
                }
            })
            .collect();
        let open = children.iter().find(|n| n.kind() == "{");
        let close = children.iter().rfind(|n| n.kind() == "}");

        if let Some(o) = open {
            self.emit_block_open(*o);
        }
        if stmts.is_empty() {
            if let Some(cl) = close {
                self.emit_verbatim(*cl);
            }
            if trailing_nl {
                self.nl();
            }
            return;
        }
        self.nl();
        self.level += 1;
        let mut prev_end_row: Option<usize> = None;
        for (stmt, trailing_semi) in &stmts {
            if let Some(prev) = prev_end_row {
                if stmt.start_position().row.saturating_sub(prev) >= 2 {
                    self.nl();
                }
            }
            self.emit_stmt_in_block(*stmt, *trailing_semi);
            prev_end_row = Some(stmt.end_position().row);
        }
        self.flush_before_close(close.copied());
        self.level -= 1;
        self.emit_indent();
        if let Some(cl) = close {
            if !cl.is_missing() {
                let t = self.text(*cl).to_string();
                self.emit(&t);
            }
        }
        if trailing_nl {
            self.nl();
        }
    }

    // Emit a statement that is a direct child of a func_block. For error/malformed nodes the
    // ";" may live as a sibling rather than inside the node; trailing_semi carries that info.
    fn emit_stmt_in_block(&mut self, node: Node, trailing_semi: bool) {
        // For compound statements (if/loops/switch/block) we always recurse so their
        // sub-structure is formatted. For simple statements, any parse error means we
        // can't safely reconstruct them, so emit verbatim.
        let is_compound = matches!(
            node.kind(),
            "if_stmt" | "while_stmt" | "do_while_stmt" | "for_stmt" | "switch_stmt" | "func_block"
        );
        if node.is_error() || (!is_compound && node.has_error()) {
            self.flush_comments_before(node.start_byte());
            let t = self.text(node).trim().to_string();
            self.emit_indent();
            self.emit(&t);
            if trailing_semi {
                self.emit(";");
            }
            self.consume_comments_before(node.end_byte());
            self.nl();
        } else {
            self.format_stmt(node);
        }
    }

    // ---- Statements ----

    fn format_stmt(&mut self, node: Node) {
        self.flush_comments_before(node.start_byte());
        if node.is_error() || node.has_error() {
            let t = self.text(node).trim().to_string();
            self.emit_indent();
            self.emit(&t);
            self.consume_comments_before(node.end_byte());
            self.nl();
            return;
        }
        match node.kind() {
            "if_stmt" => self.format_if_stmt(node),
            "while_stmt" | "do_while_stmt" | "for_stmt" => self.format_loop_stmt(node),
            "switch_stmt" => self.format_switch_stmt(node),
            "func_block" => {
                self.emit_indent();
                self.format_func_block(node);
            }
            "expr_stmt" => self.format_expr_stmt(node),
            _ => {
                self.emit_indent();
                self.format_children(node);
                self.nl();
            }
        }
    }

    pub(super) fn format_if_stmt(&mut self, node: Node) {
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

    fn emit_split_keyword_cond(&mut self, keyword_open: &str, cond: Option<Node>) -> bool {
        let Some(c) = cond else {
            return false;
        };
        let parts = split_binary_condition(c, self.source);
        if parts.len() <= 1 {
            return false;
        }
        let indent = self.level * self.indent_unit.len();
        let cond_line = indent + keyword_open.len() + self.render_node(c).len() + 1;
        if cond_line > self.line_limit || chain_fully_broken(&parts) {
            self.emit_condition_split(keyword_open, &parts);
            return true;
        }
        false
    }

    fn emit_condition_split(&mut self, keyword_open: &str, parts: &[BoolPart]) {
        self.emit_indent();
        self.emit(keyword_open);
        self.nl();
        self.level += 1;
        for part in parts {
            self.emit_indent();
            self.emit(&part.fragment);
            if let Some(op) = part.op {
                self.emit(" ");
                self.emit(op);
            }
            self.nl();
        }
        self.level -= 1;
        self.emit_indent();
        self.emit(")");
    }

    pub(super) fn try_emit_broken_chain(&mut self, node: Node, parent_kind: &str) -> bool {
        if node.kind() != "binary_op_expr" {
            return false;
        }
        // A chain nested in an enclosing chain is already rendered as one flat fragment.
        if parent_kind == "binary_op_expr" {
            return false;
        }
        let parts = split_binary_condition(node, self.source);
        if !chain_fully_broken(&parts) {
            return false;
        }
        self.level += 1;
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                self.emit_indent();
            }
            self.emit(&part.fragment);
            if let Some(op) = part.op {
                self.emit(" ");
                self.emit(op);
            }
            if i + 1 < parts.len() {
                self.nl();
            }
        }
        self.level -= 1;
        true
    }

    fn emit_stmt_body(&mut self, body: Option<Node>, layout: BodyLayout) {
        self.emit_stmt_body_trailing(body, layout, None);
    }

    // `trailing` is `Some(n)` when a continuation (do-while's `while (...)`) follows on the
    // body's last line and needs `n` more columns; the body then stays mid-line for it.
    fn emit_stmt_body_trailing(
        &mut self,
        body: Option<Node>,
        layout: BodyLayout,
        trailing: Option<usize>,
    ) {
        let mid_line = trailing.is_some();
        let Some(body) = body else {
            if mid_line {
                self.emit(" ");
            } else {
                self.nl();
            }
            return;
        };
        if body.kind() == "func_block" {
            self.emit(" ");
            self.format_func_block_inner(body, !mid_line);
            if mid_line {
                self.emit(" ");
            }
            return;
        }
        let line_len = self.current_line_len() + 1 + self.text(body).len() + trailing.unwrap_or(0);
        if layout == BodyLayout::ForceBlock || line_len > self.line_limit {
            self.emit(" {\n");
            self.level += 1;
            self.format_stmt(body);
            self.level -= 1;
            self.emit_indent();
            self.emit("}");
            if mid_line {
                self.emit(" ");
            } else {
                self.nl();
            }
        } else {
            self.emit(" ");
            self.suppress_next_indent = true;
            self.format_stmt(body);
            if mid_line && self.out.ends_with('\n') {
                self.out.pop();
                self.emit(" ");
            }
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

    pub(super) fn format_loop_stmt(&mut self, node: Node) {
        match node.kind() {
            "while_stmt" => {
                let cond = node.child_by_field_name("cond");
                let split = self.emit_split_keyword_cond("while (", cond);
                if !split {
                    self.emit_indent();
                    self.emit("while (");
                    if let Some(c) = cond {
                        self.format_node(c);
                    }
                    self.emit(")");
                }
                let body_layout = if split {
                    BodyLayout::ForceBlock
                } else {
                    BodyLayout::Auto
                };
                self.emit_stmt_body(node.child_by_field_name("body"), body_layout);
            }
            "do_while_stmt" => {
                self.emit_indent();
                self.emit("do");
                let cond = node.child_by_field_name("cond");
                let cond_len = cond.map(|c| self.render_node(c).len()).unwrap_or(0);
                let trailing = " while (".len() + cond_len + ")".len();
                self.emit_stmt_body_trailing(
                    node.child_by_field_name("body"),
                    BodyLayout::Auto,
                    Some(trailing),
                );
                self.suppress_next_indent = true;
                if !self.emit_split_keyword_cond("while (", cond) {
                    self.emit_indent();
                    self.emit("while (");
                    if let Some(c) = cond {
                        self.format_node(c);
                    }
                    self.emit(")");
                }
                self.nl();
            }
            "for_stmt" => {
                self.emit_indent();
                self.emit("for (");
                if let Some(init) = node.child_by_field_name("init") {
                    self.format_node(init);
                }
                self.emit("; ");
                if let Some(cond) = node.child_by_field_name("cond") {
                    self.format_node(cond);
                }
                self.emit("; ");
                if let Some(iter) = node.child_by_field_name("iter") {
                    self.format_node(iter);
                }
                self.emit(")");
                self.emit_stmt_body(node.child_by_field_name("body"), BodyLayout::Auto);
            }
            _ => {
                self.emit_indent();
                self.format_children(node);
                self.nl();
            }
        }
    }

    pub(super) fn format_switch_stmt(&mut self, node: Node) {
        switch::format_switch_stmt(self, node);
    }

    pub(super) fn format_expr_stmt(&mut self, node: Node) {
        self.emit_indent();
        let expr = named_child_nodes(node).into_iter().next();
        if let Some(e) = expr {
            let indent = self.level * self.indent_unit.len();
            if indent + self.render_node(e).len() + 1 > self.line_limit {
                if let Some((prefix, args)) = try_split_call_args(e, self.source) {
                    self.emit(&prefix);
                    self.emit("(\n");
                    self.level += 1;
                    for (idx, arg) in args.iter().enumerate() {
                        self.emit_indent();
                        self.emit(arg);
                        if idx + 1 < args.len() {
                            self.emit(",");
                        }
                        self.nl();
                    }
                    self.level -= 1;
                    self.emit_indent();
                    self.emit(")");
                    let semi = self.child_of_kind(node, ";");
                    if semi.map(|n| !n.is_missing()).unwrap_or(false) {
                        self.emit(";");
                    }
                    self.nl();
                    return;
                }
            }
            if !self.try_emit_broken_chain(e, node.kind()) {
                self.format_node(e);
            }
        }
        let semi = self.child_of_kind(node, ";");
        if semi.map(|n| !n.is_missing()).unwrap_or(false) {
            self.emit(";");
        }
        self.nl();
    }
}
