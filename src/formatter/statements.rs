use tree_sitter::Node;

use super::{
    child_nodes, named_child_nodes, split_binary_condition, try_split_call_args, Formatter,
};

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
                if child.is_named() && child.kind() != "nop" {
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
            if !o.is_missing() {
                let t = self.text(*o).to_string();
                self.emit(&t);
            }
        }
        if stmts.is_empty() {
            if let Some(cl) = close {
                if !cl.is_missing() {
                    let t = self.text(*cl).to_string();
                    self.emit(&t);
                }
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
            let t = self.text(node).trim().to_string();
            self.emit_indent();
            self.emit(&t);
            if trailing_semi {
                self.emit(";");
            }
            self.nl();
        } else {
            self.format_stmt(node);
        }
    }

    // ---- Statements ----

    fn format_stmt(&mut self, node: Node) {
        if node.is_error() || node.has_error() {
            let t = self.text(node).trim().to_string();
            self.emit_indent();
            self.emit(&t);
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
            "comment" => {
                let t = self.text(node).to_string();
                self.emit_indent();
                self.emit(&t);
                self.nl();
            }
            _ => {
                self.emit_indent();
                self.format_children(node);
                self.nl();
            }
        }
    }

    pub(super) fn format_if_stmt(&mut self, node: Node) {
        let force_block = self.if_chain_needs_block(node);
        self.format_if_stmt_emit(node, force_block);
    }

    fn format_if_stmt_emit(&mut self, node: Node, force_block: bool) {
        let cond = node.child_by_field_name("cond");
        let body = node.child_by_field_name("body");
        let else_body = node.child_by_field_name("else");

        let indent = self.level * self.indent_unit.len();
        let cond_len = cond.map(|c| self.render_node(c).len()).unwrap_or(0);
        let cond_line = indent + 4 + cond_len + 2;
        let cond_parts = cond
            .map(|c| split_binary_condition(c, self.source))
            .unwrap_or_default();
        let splittable_cond = cond_parts.len() > 1;

        if splittable_cond && cond_line > self.line_limit {
            self.emit_indent();
            self.emit("if (\n");
            self.level += 1;
            for (fragment, op) in cond_parts {
                self.emit_indent();
                self.emit(&fragment);
                if let Some(o) = op {
                    self.emit(" ");
                    self.emit(o);
                }
                self.nl();
            }
            self.level -= 1;
            self.emit_indent();
            self.emit(")");
            self.emit_if_body(body, true);
        } else {
            self.emit_indent();
            self.emit("if (");
            if let Some(c) = cond {
                self.format_node(c);
            }
            self.emit(")");
            self.emit_if_body(body, force_block);
        }

        if let Some(eb) = else_body {
            self.emit_indent();
            self.emit("else");
            self.emit_else_clause(eb, force_block);
        }
    }

    fn emit_if_body(&mut self, body: Option<Node>, force_block: bool) {
        match body {
            None => self.nl(),
            Some(b) if b.kind() == "func_block" => {
                self.emit(" ");
                self.format_func_block(b);
            }
            Some(b) if force_block => {
                self.emit(" {\n");
                self.level += 1;
                self.format_stmt(b);
                self.level -= 1;
                self.emit_indent();
                self.emit("}\n");
            }
            Some(b) => {
                self.emit(" ");
                self.suppress_next_indent = true;
                self.format_stmt(b);
            }
        }
    }

    fn emit_else_clause(&mut self, node: Node, force_block: bool) {
        if node.kind() == "if_stmt" {
            self.emit(" ");
            self.suppress_next_indent = true;
            self.format_if_stmt_emit(node, force_block);
        } else if node.kind() == "func_block" {
            self.emit(" ");
            self.format_func_block(node);
        } else if force_block {
            self.emit(" {\n");
            self.level += 1;
            self.format_stmt(node);
            self.level -= 1;
            self.emit_indent();
            self.emit("}\n");
        } else {
            self.emit(" ");
            self.suppress_next_indent = true;
            self.format_stmt(node);
        }
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

    fn emit_compound_body(&mut self, node: Node) {
        if node.kind() == "func_block" {
            self.emit(" ");
            self.format_func_block(node);
        } else {
            self.nl();
            self.level += 1;
            self.format_stmt(node);
            self.level -= 1;
        }
    }

    pub(super) fn format_loop_stmt(&mut self, node: Node) {
        match node.kind() {
            "while_stmt" => {
                self.emit_indent();
                self.emit("while (");
                if let Some(cond) = node.child_by_field_name("cond") {
                    self.format_node(cond);
                }
                self.emit(")");
                if let Some(b) = node.child_by_field_name("body") {
                    self.emit_compound_body(b);
                } else {
                    self.nl();
                }
            }
            "do_while_stmt" => {
                self.emit_indent();
                self.emit("do");
                if let Some(b) = node.child_by_field_name("body") {
                    if b.kind() == "func_block" {
                        self.emit(" ");
                        self.format_func_block_inner(b, false);
                        self.emit(" while (");
                    } else {
                        self.nl();
                        self.level += 1;
                        self.format_stmt(b);
                        self.level -= 1;
                        self.emit_indent();
                        self.emit("while (");
                    }
                } else {
                    self.emit(" while (");
                }
                if let Some(cond) = node.child_by_field_name("cond") {
                    self.format_node(cond);
                }
                self.emit(")\n");
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
                if let Some(b) = node.child_by_field_name("body") {
                    self.emit_compound_body(b);
                } else {
                    self.nl();
                }
            }
            _ => {
                self.emit_indent();
                self.format_children(node);
                self.nl();
            }
        }
    }

    pub(super) fn format_switch_stmt(&mut self, node: Node) {
        self.emit_indent();
        self.emit("switch (");
        if let Some(cond) = node.child_by_field_name("cond") {
            self.format_node(cond);
        }
        self.emit(") {\n");
        self.level += 1;
        if let Some(block) = self.child_of_kind(node, "switch_block") {
            let children = child_nodes(block);
            for child in &children {
                match child.kind() {
                    "switch_case_label" | "switch_default_label" => {
                        self.level -= 1;
                        self.emit_indent();
                        self.level += 1;
                        self.format_children(*child);
                        self.nl();
                    }
                    _ if child.is_named() => self.format_stmt(*child),
                    _ => {}
                }
            }
        }
        self.level -= 1;
        self.emit_indent();
        self.emit("}\n");
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
            self.format_node(e);
        }
        let semi = self.child_of_kind(node, ";");
        if semi.map(|n| !n.is_missing()).unwrap_or(false) {
            self.emit(";");
        }
        self.nl();
    }
}
