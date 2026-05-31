use tree_sitter::Node;

use super::{child_nodes, is_expr_node, Formatter};

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommentPlacement {
    Trailing,
    OwnLine,
}

// `//` runs to end-of-line: it must be newline-terminated or it swallows the next token.
fn is_line_comment(text: &str) -> bool {
    text.trim_start().starts_with("//")
}

fn comment_placement(prev: Option<Node>, comment: Node) -> CommentPlacement {
    match prev {
        Some(p) if p.end_position().row == comment.start_position().row => {
            CommentPlacement::Trailing
        }
        _ => CommentPlacement::OwnLine,
    }
}

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

    // Emit comments starting before `byte`, so none is stranded behind a later token.
    pub(super) fn flush_comments_before(&mut self, byte: usize) {
        while self
            .comments
            .get(self.comment_cursor)
            .is_some_and(|c| c.start_byte() < byte)
        {
            let comment = self.comments[self.comment_cursor];
            self.comment_cursor += 1;
            self.emit_comment(comment);
        }
    }

    pub(super) fn is_trailing_comment(&self, prev: Option<Node>, comment: Node) -> bool {
        comment_placement(prev, comment) == CommentPlacement::Trailing
    }

    pub(super) fn flush_before_close(&mut self, close: Option<Node>) {
        if let Some(cl) = close {
            self.flush_comments_before(cl.start_byte());
        }
    }

    // Skip comments before `byte` whose text was already emitted verbatim by the caller.
    pub(super) fn consume_comments_before(&mut self, byte: usize) {
        while self
            .comments
            .get(self.comment_cursor)
            .is_some_and(|c| c.start_byte() < byte)
        {
            self.comment_cursor += 1;
        }
    }

    pub(super) fn emit_verbatim(&mut self, node: Node) {
        if !node.is_missing() {
            self.flush_comments_before(node.start_byte());
            let t = self.text(node).to_string();
            self.emit(&t);
            self.consume_comments_before(node.end_byte());
        }
    }

    // True when a trailing comment can rejoin the current line: mid-line, or a lone
    // '\n' right after content (not a blank line, where rejoining would dangle).
    fn can_trail(&self) -> bool {
        if self.out.is_empty() {
            return false;
        }
        if !self.out.ends_with('\n') {
            return true;
        }
        let before = &self.out[..self.out.len() - 1];
        !before.is_empty() && !before.ends_with('\n')
    }

    fn emit_comment(&mut self, comment: Node) {
        let prev = comment.prev_sibling();
        let text = self.text(comment).trim_end().to_string();
        let line_comment = is_line_comment(&text);
        if comment_placement(prev, comment) == CommentPlacement::Trailing && self.can_trail() {
            let popped = self.out.ends_with('\n');
            if popped {
                self.out.pop();
            }
            if !self.out.ends_with(' ') {
                self.emit(" ");
            }
            self.emit(&text);
            // `//` runs to EOL; a rejoined `/* */` must restore the row break we popped.
            if popped || line_comment {
                self.nl();
            }
            return;
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.nl();
        }
        self.emit_indent();
        self.emit(&text);
        self.nl();
    }

    pub(super) fn format_node(&mut self, node: Node) {
        if node.is_missing() {
            return;
        }
        self.flush_comments_before(node.start_byte());
        if node.is_error() {
            let t = self.text(node).trim().to_string();
            self.emit(&t);
            self.consume_comments_before(node.end_byte());
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
            self.flush_comments_before(child.start_byte());
            if child.kind() == "comment" {
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
                // A preceding comment may have ended the line; don't prefix a space.
                if !self.out.ends_with('\n') && self.gap_between(p, *child, node.kind()) {
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
