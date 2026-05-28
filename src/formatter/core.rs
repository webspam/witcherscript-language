use tree_sitter::Node;

use super::{child_nodes, is_expr_node, Formatter};

impl<'a> Formatter<'a> {
    pub(super) fn text(&self, node: Node) -> &'a str {
        &self.source[node.start_byte()..node.end_byte()]
    }

    pub(super) fn emit(&mut self, s: &str) {
        self.out.push_str(s);
    }

    pub(super) fn emit_indent(&mut self) {
        if self.suppress_next_indent {
            self.suppress_next_indent = false;
            return;
        }
        for _ in 0..self.level {
            let unit = self.indent_unit.clone();
            self.out.push_str(&unit);
        }
    }

    pub(super) fn nl(&mut self) {
        self.out.push('\n');
    }

    pub(super) fn child_of_kind<'t>(&self, node: Node<'t>, kind: &str) -> Option<Node<'t>> {
        let mut c = node.walk();
        let result = node.children(&mut c).find(|n| n.kind() == kind);
        result
    }

    pub(super) fn current_line_len(&self) -> usize {
        let last_nl = self.out.rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.out[last_nl..].len()
    }

    // ---- Core: token-preserving walk ----

    // Universal safety-net: emit a node's source text verbatim.
    // Use this as the fallback in every exhaustive child loop so that
    // no CST node — especially comment extras — is ever silently dropped.
    pub(super) fn emit_verbatim(&mut self, node: Node) {
        if !node.is_missing() {
            let t = self.text(node).to_string();
            self.emit(&t);
        }
    }

    pub(super) fn format_node(&mut self, node: Node) {
        if node.is_missing() {
            return;
        }
        if node.is_error() {
            let t = self.text(node).trim().to_string();
            self.emit(&t);
            return;
        }
        if node.child_count() == 0 {
            let t = self.text(node).to_string();
            self.emit(&t);
            return;
        }
        match node.kind() {
            "script" => self.format_script(node),
            "func_decl" | "event_decl" => self.format_func_decl(node),
            "class_decl" | "struct_decl" | "state_decl" => self.format_class_decl(node),
            "enum_decl" => self.format_enum_decl(node),
            "member_var_decl" => self.format_member_var_decl(node, None, None),
            "class_def" | "struct_def" => self.format_class_def(node),
            "func_block" => self.format_func_block(node),
            "if_stmt" => self.format_if_stmt(node),
            "while_stmt" | "do_while_stmt" | "for_stmt" => self.format_loop_stmt(node),
            "switch_stmt" => self.format_switch_stmt(node),
            "expr_stmt" => self.format_expr_stmt(node),
            _ if is_expr_node(node.kind()) => self.format_children(node),
            _ => self.format_children(node),
        }
    }

    pub(super) fn format_children(&mut self, node: Node) {
        let children = child_nodes(node);
        let mut prev: Option<Node> = None;
        for child in &children {
            if child.is_missing() || child.kind() == "annotation" {
                continue;
            }
            if child.kind() == ":" {
                if let Some(col) = self.colon_align_col.take() {
                    let mut len = self.current_line_len();
                    while len < col {
                        self.emit(" ");
                        len += 1;
                    }
                }
            }
            if let Some(p) = prev {
                if self.gap_between(p, *child, node.kind()) {
                    self.emit(" ");
                }
            }
            if child.child_count() == 0 {
                self.emit_verbatim(*child);
            } else {
                self.format_node(*child);
            }
            prev = Some(*child);
        }
    }

    pub(super) fn gap_between(&self, before: Node, after: Node, parent_kind: &str) -> bool {
        let bk = before.kind();
        let ak = after.kind();
        if matches!(ak, "," | ";" | ")" | "]" | "<" | ">") {
            return false;
        }
        if matches!(bk, "(" | "[" | "<") {
            return false;
        }
        if parent_kind == "cast_expr" && bk == ")" {
            return false;
        }
        if parent_kind == "unary_op_expr"
            && matches!(
                bk,
                "unary_op_not" | "unary_op_neg" | "unary_op_bitnot" | "unary_op_plus"
            )
        {
            return false;
        }
        if ak == "func_params" {
            return false;
        }
        if ak == "(" || ak == "[" {
            return false;
        }
        if ak == "." || bk == "." {
            return false;
        }
        if ak == ":" {
            match parent_kind {
                "switch_case_label" | "switch_default_label" => return false,
                "local_var_decl_stmt"
                | "member_var_decl"
                | "func_param_group"
                | "event_decl"
                | "func_decl"
                | "autobind_decl" => return !self.compact_colon,
                _ => {}
            }
        }
        true
    }

    // Render a subtree to a String (no indentation/newlines) — used for line-length measurement
    pub(super) fn render_node(&self, node: Node) -> String {
        if node.is_missing() {
            return String::new();
        }
        if node.is_error() {
            return self.text(node).trim().to_string();
        }
        if node.child_count() == 0 {
            return self.text(node).to_string();
        }
        let children = child_nodes(node);
        let mut s = String::new();
        let mut prev: Option<Node> = None;
        for child in &children {
            if child.is_missing() {
                continue;
            }
            if let Some(p) = prev {
                if self.gap_between(p, *child, node.kind()) {
                    s.push(' ');
                }
            }
            if child.child_count() == 0 {
                s.push_str(self.text(*child));
            } else {
                s.push_str(&self.render_node(*child));
            }
            prev = Some(*child);
        }
        s
    }
}
