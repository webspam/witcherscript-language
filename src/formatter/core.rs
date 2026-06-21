use tree_sitter::Node;

use crate::cst::kinds;

use super::{ColonSpacing, Formatter, child_nodes, comment_in_range, is_expr_node};

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

        node.children(&mut c).find(|n| n.kind() == kind)
    }

    pub(super) fn current_line_len(&self) -> usize {
        let last_nl = self.out.rfind('\n').map_or(0, |i| i + 1);
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

    // Flush comments trailing `row` before a blank-line decision can strand them onto their own line.
    pub(super) fn flush_trailing_comments(&mut self, row: usize, byte: usize) {
        while self
            .comments
            .get(self.comment_cursor)
            .is_some_and(|c| c.start_byte() < byte && c.start_position().row == row)
        {
            let comment = self.comments[self.comment_cursor];
            self.comment_cursor += 1;
            self.emit_comment(comment);
        }
    }

    pub(super) fn is_trailing_comment(&self, prev: Option<Node>, comment: Node) -> bool {
        comment_placement(prev, comment) == CommentPlacement::Trailing
    }

    pub(super) fn has_interior_comment(&self, node: Node) -> bool {
        comment_in_range(&self.comments, node.start_byte(), node.end_byte())
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

    // A preceding `//` comment would swallow the brace, so move it to its own indented line.
    pub(super) fn emit_block_open(&mut self, open: Node) {
        if open.is_missing() {
            return;
        }
        self.flush_comments_before(open.start_byte());
        if self.out.ends_with('\n') {
            self.emit_indent();
        } else if !self.out.ends_with(' ') {
            self.emit(" ");
        }
        let t = self.text(open).to_string();
        self.emit(&t);
        self.consume_comments_before(open.end_byte());
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
            let t = self.original_node_text(node);
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
            kinds::SCRIPT => self.format_script(node),
            kinds::FUNC_DECL | kinds::EVENT_DECL => self.format_func_decl(node),
            kinds::CLASS_DECL | kinds::STRUCT_DECL | kinds::STATE_DECL => {
                self.format_class_decl(node);
            }
            kinds::ENUM_DECL => self.format_enum_decl(node),
            kinds::MEMBER_VAR_DECL => self.format_member_var_decl(node, None, None),
            kinds::CLASS_DEF | kinds::STRUCT_DEF => self.format_class_def(node),
            kinds::FUNC_BLOCK => self.format_func_block(node),
            kinds::IF_STMT => self.format_if_stmt(node),
            kinds::WHILE_STMT | kinds::DO_WHILE_STMT | kinds::FOR_STMT => {
                self.format_loop_stmt(node);
            }
            kinds::SWITCH_STMT => self.format_switch_stmt(node),
            kinds::EXPR_STMT => self.format_expr_stmt(node),
            _ if is_expr_node(node.kind()) => self.format_children(node),
            _ => self.format_children(node),
        }
    }

    pub(super) fn format_children(&mut self, node: Node) {
        let children = child_nodes(node);
        let mut prev: Option<Node> = None;
        for child in &children {
            if child.is_missing() || child.kind() == kinds::ANNOTATION {
                continue;
            }
            self.flush_comments_before(child.start_byte());
            if child.kind() == kinds::COMMENT {
                continue;
            }
            if child.kind() == ":"
                && let Some(col) = self.colon_align_col.take()
            {
                let mut len = self.current_line_len();
                while len < col {
                    self.emit(" ");
                    len += 1;
                }
            }
            if let Some(p) = prev {
                if self.out.ends_with('\n') {
                    // A `//` comment forced a mid-statement break; indent the continuation.
                    self.level += 1;
                    self.emit_indent();
                    self.level -= 1;
                } else if self.gap_between(p, *child, node.kind()) {
                    self.emit(" ");
                }
            }
            if child.child_count() == 0 {
                self.emit_verbatim(*child);
            } else if !self.try_emit_broken_chain(*child, node.kind()) {
                self.format_node(*child);
            }
            prev = Some(*child);
        }
    }

    pub(super) fn gap_between(&self, before: Node, after: Node, parent_kind: &str) -> bool {
        let bk = before.kind();
        let ak = after.kind();
        // An empty call argument is two adjacent commas; render the gap as one space.
        if parent_kind == kinds::FUNC_CALL_ARGS && bk == "," && ak == "," {
            return true;
        }
        if matches!(ak, "," | ";" | ")" | "]" | "<" | ">") {
            return false;
        }
        if matches!(bk, "(" | "[" | "<") {
            return false;
        }
        if parent_kind == kinds::CAST_EXPR && bk == ")" {
            return false;
        }
        if parent_kind == kinds::UNARY_OP_EXPR
            && matches!(
                bk,
                kinds::UNARY_OP_NOT
                    | kinds::UNARY_OP_NEG
                    | kinds::UNARY_OP_BITNOT
                    | kinds::UNARY_OP_PLUS
            )
        {
            return false;
        }
        if ak == kinds::FUNC_PARAMS {
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
                kinds::SWITCH_CASE_LABEL | kinds::SWITCH_DEFAULT_LABEL => return false,
                kinds::LOCAL_VAR_DECL_STMT
                | kinds::MEMBER_VAR_DECL
                | kinds::FUNC_PARAM_GROUP
                | kinds::EVENT_DECL
                | kinds::FUNC_DECL
                | kinds::AUTOBIND_DECL => return self.colon == ColonSpacing::Spaced,
                _ => {}
            }
        }
        true
    }

    // Render a subtree to a String (no indentation/newlines) - used for line-length measurement
    pub(super) fn render_node(&self, node: Node) -> String {
        if node.is_missing() {
            return String::new();
        }
        if node.is_error() {
            return self.original_node_text(node);
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
            if let Some(p) = prev
                && self.gap_between(p, *child, node.kind())
            {
                s.push(' ');
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

    pub(super) fn original_node_text(&self, node: Node) -> String {
        self.text(node).trim().to_string()
    }
}
