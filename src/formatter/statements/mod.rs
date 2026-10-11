mod if_stmt;
mod switch;

use tree_sitter::Node;

use crate::cst::{fields, kinds};

pub(in crate::formatter) use if_stmt::{block_single_stmt, body_expandable, chain_bodies};
pub(in crate::formatter) use switch::{SwitchArm, collect_switch_arms};

use super::{
    ChainPart, Formatter, OverflowWrapTarget, blank_line_between_rows, chain_fully_broken,
    child_nodes, named_child_nodes, split_binary_chain, splittable_call,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::formatter) enum BodyLayout {
    Auto,
    ForceBlock,
}

impl Formatter<'_> {
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
                if child.is_named() && child.kind() != kinds::NOP && child.kind() != kinds::COMMENT
                {
                    let trailing_semi = children
                        .get(i + 1)
                        .is_some_and(|n| n.kind() == ";" || n.kind() == kinds::NOP);
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
            // Attach a trailing comment to prev's line before its row is read as the gap target.
            if let Some(prev) = prev_end_row {
                self.flush_trailing_comments(prev, stmt.start_byte());
            }
            prev_end_row = self.flush_own_line_comments(stmt.start_byte(), prev_end_row);
            if let Some(prev) = prev_end_row
                && blank_line_between_rows(prev, stmt.start_position().row)
            {
                self.nl();
            }
            self.emit_stmt_in_block(*stmt, *trailing_semi);
            prev_end_row = Some(stmt.end_position().row);
        }
        self.flush_before_close(close.copied());
        self.level -= 1;
        self.emit_indent();
        if let Some(cl) = close
            && !cl.is_missing()
        {
            let t = self.text(*cl).to_string();
            self.emit(&t);
        }
        if trailing_nl {
            self.nl();
        }
    }

    // Per-comment gap handling so a blank after a comment survives like one between statements.
    fn flush_own_line_comments(
        &mut self,
        byte: usize,
        mut prev_end_row: Option<usize>,
    ) -> Option<usize> {
        while let Some(comment) = self.comments.get(self.comment_cursor).copied() {
            if comment.start_byte() >= byte {
                break;
            }
            if let Some(prev) = prev_end_row
                && blank_line_between_rows(prev, comment.start_position().row)
            {
                self.nl();
            }
            self.flush_comments_before(comment.end_byte());
            prev_end_row = Some(comment.end_position().row);
        }
        prev_end_row
    }

    // Emit a statement that is a direct child of a func_block. For error/malformed nodes the
    // ";" may live as a sibling rather than inside the node; trailing_semi carries that info.
    fn emit_stmt_in_block(&mut self, node: Node, trailing_semi: bool) {
        // For compound statements (if/loops/switch/block) we always recurse so their
        // sub-structure is formatted. For simple statements, any parse error means we
        // can't safely reconstruct them, so emit verbatim.
        let is_compound = matches!(
            node.kind(),
            kinds::IF_STMT
                | kinds::WHILE_STMT
                | kinds::DO_WHILE_STMT
                | kinds::FOR_STMT
                | kinds::SWITCH_STMT
                | kinds::FUNC_BLOCK
        );
        if node.is_error() || (!is_compound && node.has_error()) {
            self.flush_comments_before(node.start_byte());
            let t = self.original_node_text(node);
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
            let t = self.original_node_text(node);
            self.emit_indent();
            self.emit(&t);
            self.consume_comments_before(node.end_byte());
            self.nl();
            return;
        }
        match node.kind() {
            kinds::IF_STMT => self.format_if_stmt(node),
            kinds::WHILE_STMT | kinds::DO_WHILE_STMT | kinds::FOR_STMT => {
                self.format_loop_stmt(node);
            }
            kinds::SWITCH_STMT => self.format_switch_stmt(node),
            kinds::FUNC_BLOCK => {
                self.emit_indent();
                self.format_func_block(node);
            }
            kinds::EXPR_STMT => self.format_expr_stmt(node),
            _ => {
                self.emit_indent();
                self.with_overflow_wrap_target(node, |f| f.format_children(node));
                self.nl();
            }
        }
    }

    fn with_overflow_wrap_target(&mut self, stmt: Node, format: impl FnOnce(&mut Self)) {
        let trailing_len = self
            .child_of_kind(stmt, ";")
            .filter(|semi| !semi.is_missing())
            .map_or(0, |semi| semi.byte_range().len());
        self.overflow_wrap_target = statement_value(stmt).map(|value| OverflowWrapTarget {
            value_id: value.id(),
            trailing_len,
        });
        format(self);
        self.overflow_wrap_target = None;
    }

    fn emit_split_keyword_cond(&mut self, keyword_open: &str, cond: Option<Node>) -> bool {
        let Some(c) = cond else {
            return false;
        };
        let parts = split_binary_chain(c);
        if parts.len() <= 1 {
            return false;
        }
        let indent = self.level * self.indent_unit.len();
        let cond_line = indent + keyword_open.len() + self.render_node(c).len() + 1;
        if cond_line > self.line_limit || chain_fully_broken(&parts) {
            self.emit_condition_split(keyword_open, c, &parts);
            return true;
        }
        false
    }

    fn emit_condition_split(&mut self, keyword_open: &str, cond: Node, parts: &[ChainPart]) {
        self.emit_indent();
        self.emit(keyword_open);
        self.nl();
        self.level += 1;
        for part in parts {
            // Do not move interleaved comments
            self.flush_comments_before(part.operand.start_byte());
            self.emit_indent();
            self.format_node(part.operand);
            if let Some(op) = part.op {
                self.emit(" ");
                self.emit(op);
            }
            self.nl();
        }
        self.level -= 1;
        if let Some(close) = close_paren_after(cond) {
            self.flush_comments_before(close.start_byte());
        }
        self.emit_indent();
        self.emit(")");
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
        if body.kind() == kinds::FUNC_BLOCK {
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

    pub(super) fn format_loop_stmt(&mut self, node: Node) {
        match node.kind() {
            kinds::WHILE_STMT => {
                let cond = node.child_by_field_name(fields::COND);
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
                self.emit_stmt_body(node.child_by_field_name(fields::BODY), body_layout);
            }
            kinds::DO_WHILE_STMT => {
                self.emit_indent();
                self.emit("do");
                let cond = node.child_by_field_name(fields::COND);
                let cond_len = cond.map_or(0, |c| self.render_node(c).len());
                let trailing = " while (".len() + cond_len + ")".len();
                self.emit_stmt_body_trailing(
                    node.child_by_field_name(fields::BODY),
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
            kinds::FOR_STMT => {
                self.emit_indent();
                self.emit("for (");
                if let Some(init) = node.child_by_field_name(fields::INIT) {
                    self.format_node(init);
                }
                self.emit("; ");
                if let Some(cond) = node.child_by_field_name(fields::COND) {
                    self.format_node(cond);
                }
                self.emit("; ");
                if let Some(iter) = node.child_by_field_name(fields::ITER) {
                    self.format_node(iter);
                }
                self.emit(")");
                self.emit_stmt_body(node.child_by_field_name(fields::BODY), BodyLayout::Auto);
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
            if indent + self.render_node(e).len() + 1 > self.line_limit
                && let Some((lead, func, args)) = self.splittable_stmt_call(e)
            {
                if let Some(lead) = lead {
                    self.emit(&lead);
                }
                self.emit_wrapped_call(func, args, node);
                return;
            }
            self.with_overflow_wrap_target(node, |f| f.format_operand(e, node.kind()));
        }
        let semi = self.child_of_kind(node, ";");
        if semi.is_some_and(|n| !n.is_missing()) {
            self.emit(";");
        }
        self.nl();
    }

    fn splittable_stmt_call<'tree>(
        &self,
        e: Node<'tree>,
    ) -> Option<(Option<String>, Node<'tree>, Node<'tree>)> {
        if let Some((func, args)) = splittable_call(e) {
            return Some((None, func, args));
        }
        if e.kind() != kinds::ASSIGN_OP_EXPR {
            return None;
        }
        let rhs = e.child_by_field_name(fields::RIGHT)?;
        let (func, args) = splittable_call(rhs)?;
        let lhs = e.child_by_field_name(fields::LEFT)?;
        let op = e.child_by_field_name(fields::OP)?;
        let lead = format!("{} {} ", self.render_node(lhs), self.render_node(op));
        Some((Some(lead), func, args))
    }

    fn emit_wrapped_call(&mut self, func: Node, args: Node, stmt: Node) {
        let prefix = self.render_node(func);
        self.emit(&prefix);
        self.emit("(");
        self.nl();
        self.level += 1;
        self.emit_call_arg_lines(args);
        self.level -= 1;
        if !self.out.ends_with('\n') {
            self.nl();
        }
        self.emit_indent();
        self.emit(")");
        if self
            .child_of_kind(stmt, ";")
            .is_some_and(|n| !n.is_missing())
        {
            self.emit(";");
        }
        self.nl();
    }

    // An omitted slot (two adjacent commas) must survive as a lone comma line, not vanish.
    fn emit_call_arg_lines(&mut self, args: Node) {
        self.emit_indent();
        let mut pending_break = false;
        for child in &child_nodes(args) {
            match child.kind() {
                // Flush, don't render: the cursor emits the comment once and consumes it, so the
                // end-of-statement sweep can't print it again.
                kinds::COMMENT => self.flush_comments_before(child.end_byte()),
                "," => {
                    if pending_break {
                        self.break_to_indented_line();
                    }
                    self.emit(",");
                    pending_break = true;
                }
                _ => {
                    if pending_break {
                        self.break_to_indented_line();
                        pending_break = false;
                    }
                    self.flush_comments_before(child.start_byte());
                    self.format_operand(*child, args.kind());
                }
            }
        }
    }
}

fn statement_value(stmt: Node) -> Option<Node> {
    match stmt.kind() {
        kinds::RETURN_STMT => named_child_nodes(stmt)
            .into_iter()
            .find(|child| child.kind() != kinds::COMMENT),
        kinds::LOCAL_VAR_DECL_STMT => stmt.child_by_field_name(fields::INIT_VALUE),
        kinds::EXPR_STMT => named_child_nodes(stmt)
            .into_iter()
            .find(|child| child.kind() == kinds::ASSIGN_OP_EXPR)
            .and_then(|assign| assign.child_by_field_name(fields::RIGHT)),
        _ => None,
    }
}

fn close_paren_after(cond: Node) -> Option<Node> {
    let parent = cond.parent()?;
    let mut cursor = parent.walk();
    parent
        .children(&mut cursor)
        .find(|n| n.kind() == ")" && n.start_byte() >= cond.end_byte())
}
