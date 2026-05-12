use tree_sitter::Node;

fn normalize_expr(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut prev_non_space: u8 = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => {
                out.push(b'"');
                i += 1;
                while i < bytes.len() {
                    let sb = bytes[i];
                    out.push(sb);
                    i += 1;
                    if sb == b'\\' && i < bytes.len() {
                        out.push(bytes[i]);
                        i += 1;
                    } else if sb == b'"' {
                        break;
                    }
                }
                prev_non_space = b'"';
            }
            b'\'' => {
                out.push(b'\'');
                i += 1;
                while i < bytes.len() {
                    let sb = bytes[i];
                    out.push(sb);
                    i += 1;
                    if sb == b'\'' {
                        break;
                    }
                }
                prev_non_space = b'\'';
            }
            b if b.is_ascii_whitespace() => {
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= bytes.len() {
                    break;
                }
                let next = bytes[i];
                let after_ident = prev_non_space.is_ascii_alphanumeric()
                    || matches!(prev_non_space, b'_' | b')' | b']' | b'"' | b'\'');
                let remove = (next == b'(' && after_ident)
                    || prev_non_space == b'('
                    || next == b')'
                    || next == b'.'
                    || prev_non_space == b'.';
                if !remove {
                    out.push(b' ');
                }
            }
            b => {
                out.push(b);
                prev_non_space = b;
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn split_binary_condition(s: &str) -> Vec<(String, Option<&'static str>)> {
    let mut parts: Vec<(String, Option<&'static str>)> = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' => {
                depth += 1;
                current.push(bytes[i] as char);
                i += 1;
            }
            b')' | b']' => {
                depth -= 1;
                current.push(bytes[i] as char);
                i += 1;
            }
            b'"' => {
                current.push('"');
                i += 1;
                while i < bytes.len() {
                    let sb = bytes[i];
                    current.push(sb as char);
                    i += 1;
                    if sb == b'\\' && i < bytes.len() {
                        current.push(bytes[i] as char);
                        i += 1;
                    } else if sb == b'"' {
                        break;
                    }
                }
            }
            b'\'' => {
                current.push('\'');
                i += 1;
                while i < bytes.len() {
                    let sb = bytes[i];
                    current.push(sb as char);
                    i += 1;
                    if sb == b'\'' {
                        break;
                    }
                }
            }
            b'|' if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                parts.push((current.trim_end().to_string(), Some("||")));
                current = String::new();
                i += 2;
                while i < bytes.len() && bytes[i] == b' ' {
                    i += 1;
                }
            }
            b'&' if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                parts.push((current.trim_end().to_string(), Some("&&")));
                current = String::new();
                i += 2;
                while i < bytes.len() && bytes[i] == b' ' {
                    i += 1;
                }
            }
            b => {
                current.push(b as char);
                i += 1;
            }
        }
    }

    let last = current.trim().to_string();
    if !last.is_empty() {
        parts.push((last, None));
    }
    parts
}

fn split_at_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' => {
                depth += 1;
                current.push(bytes[i] as char);
                i += 1;
            }
            b')' | b']' => {
                depth -= 1;
                current.push(bytes[i] as char);
                i += 1;
            }
            b'"' => {
                current.push('"');
                i += 1;
                while i < bytes.len() {
                    let sb = bytes[i];
                    current.push(sb as char);
                    i += 1;
                    if sb == b'\\' && i < bytes.len() {
                        current.push(bytes[i] as char);
                        i += 1;
                    } else if sb == b'"' {
                        break;
                    }
                }
            }
            b'\'' => {
                current.push('\'');
                i += 1;
                while i < bytes.len() {
                    let sb = bytes[i];
                    current.push(sb as char);
                    i += 1;
                    if sb == b'\'' {
                        break;
                    }
                }
            }
            b',' if depth == 0 => {
                parts.push(current.trim().to_string());
                current = String::new();
                i += 1;
                while i < bytes.len() && bytes[i] == b' ' {
                    i += 1;
                }
            }
            b => {
                current.push(b as char);
                i += 1;
            }
        }
    }
    let last = current.trim().to_string();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

fn try_split_call_args(s: &str) -> Option<(String, Vec<String>)> {
    if !s.ends_with(')') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut open = None;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    open = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let open = open?;
    if open == 0 {
        return None;
    }
    let prefix = s[..open].to_string();
    let args_str = &s[open + 1..s.len() - 1];
    let args = split_at_commas(args_str);
    if args.len() <= 1 {
        return None;
    }
    Some((prefix, args))
}

fn is_expr_node(kind: &str) -> bool {
    matches!(
        kind,
        "array_init_expr"
            | "assign_op_expr"
            | "ternary_cond_expr"
            | "binary_op_expr"
            | "new_expr"
            | "unary_op_expr"
            | "cast_expr"
            | "member_access_expr"
            | "incomplete_member_access_expr"
            | "func_call_expr"
            | "array_expr"
            | "nested_expr"
            | "this_expr"
            | "super_expr"
            | "parent_expr"
            | "virtual_parent_expr"
            | "literal_null"
            | "literal_float"
            | "literal_int"
            | "literal_hex"
            | "literal_bool"
            | "literal_string"
            | "literal_name"
    )
}

pub fn format_document(
    root: Node,
    source: &str,
    tab_size: u32,
    use_tabs: bool,
    line_limit: u32,
    compact_colon: bool,
) -> String {
    let indent_unit = if use_tabs {
        "\t".to_string()
    } else {
        " ".repeat(tab_size as usize)
    };
    let mut f = Formatter {
        source,
        indent_unit,
        level: 0,
        out: String::with_capacity(source.len()),
        suppress_next_indent: false,
        line_limit: line_limit as usize,
        compact_colon,
    };
    f.format_node(root);
    while f.out.ends_with("\n\n") {
        f.out.pop();
    }
    if !f.out.ends_with('\n') {
        f.out.push('\n');
    }
    f.out
}

struct Formatter<'a> {
    source: &'a str,
    indent_unit: String,
    level: usize,
    out: String,
    suppress_next_indent: bool,
    line_limit: usize,
    compact_colon: bool,
}

impl<'a> Formatter<'a> {
    fn text(&self, node: Node) -> &'a str {
        &self.source[node.start_byte()..node.end_byte()]
    }

    fn emit(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn emit_indent(&mut self) {
        if self.suppress_next_indent {
            self.suppress_next_indent = false;
            return;
        }
        for _ in 0..self.level {
            let unit = self.indent_unit.clone();
            self.out.push_str(&unit);
        }
    }

    fn nl(&mut self) {
        self.out.push('\n');
    }

    fn child_of_kind<'t>(&self, node: Node<'t>, kind: &str) -> Option<Node<'t>> {
        let mut c = node.walk();
        let result = node.children(&mut c).find(|n| n.kind() == kind);
        result
    }

    fn current_line_len(&self) -> usize {
        let last_nl = self.out.rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.out[last_nl..].len()
    }

    // ---- Core: token-preserving walk ----

    // Universal safety-net: emit a node's source text verbatim.
    // Use this as the fallback in every exhaustive child loop so that
    // no CST node — especially comment extras — is ever silently dropped.
    fn emit_verbatim(&mut self, node: Node) {
        if !node.is_missing() {
            let t = self.text(node).to_string();
            self.emit(&t);
        }
    }

    fn format_node(&mut self, node: Node) {
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
            "class_decl" | "struct_decl" => self.format_class_decl(node),
            "state_decl" => self.format_state_decl(node),
            "enum_decl" => self.format_enum_decl(node),
            "class_def" | "struct_def" => self.format_class_def(node),
            "func_block" => self.format_func_block(node),
            "if_stmt" => self.format_if_stmt(node),
            "while_stmt" | "do_while_stmt" | "for_stmt" => self.format_loop_stmt(node),
            "switch_stmt" => self.format_switch_stmt(node),
            "expr_stmt" => self.format_expr_stmt(node),
            _ if is_expr_node(node.kind()) => {
                let t = normalize_expr(self.text(node));
                self.emit(&t);
            }
            _ => self.format_children(node),
        }
    }

    fn format_children(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
        let mut prev: Option<Node> = None;
        for child in &children {
            if child.is_missing() {
                continue;
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

    fn gap_between(&self, before: Node, after: Node, parent_kind: &str) -> bool {
        let bk = before.kind();
        let ak = after.kind();
        if matches!(ak, "," | ";" | ")" | "]" | ">") {
            return false;
        }
        if matches!(bk, "(" | "[" | "<") {
            return false;
        }
        if ak == "func_params" {
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
    fn render_node(&self, node: Node) -> String {
        if node.is_missing() {
            return String::new();
        }
        if node.is_error() {
            return self.text(node).trim().to_string();
        }
        if node.child_count() == 0 {
            return self.text(node).to_string();
        }
        if is_expr_node(node.kind()) {
            return normalize_expr(self.text(node));
        }
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
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

    // ---- Top level ----

    fn format_script(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c)
                .filter(|n| n.is_named() && n.kind() != "nop")
                .collect()
        };
        let mut prev_end_row: Option<usize> = None;
        let mut prev_was_comment = false;
        for child in &children {
            let child_row = child.start_position().row;
            if let Some(prev) = prev_end_row {
                let source_gap = child_row.saturating_sub(prev);
                if source_gap >= 2 || !prev_was_comment {
                    self.nl();
                }
            }
            prev_was_comment = child.kind() == "comment";
            if child.kind() == "comment" {
                let t = self.text(*child).to_string();
                self.emit_indent();
                self.emit(&t);
                self.nl();
            } else {
                self.format_node(*child);
            }
            prev_end_row = Some(child.end_position().row);
        }
    }

    // ---- Declarations ----

    fn format_func_decl(&mut self, node: Node) {
        if let Some(ann) = self.child_of_kind(node, "annotation") {
            let t = self.text(ann).to_string();
            self.emit_indent();
            self.emit(&t);
            self.nl();
        }
        self.emit_indent();

        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
        let mut prev: Option<Node> = None;
        let mut emitted_sig = false;
        for child in &children {
            if child.is_missing() {
                continue;
            }
            match child.kind() {
                "annotation" => continue,
                "func_params" => {
                    if !emitted_sig {
                        self.format_func_sig(node);
                        emitted_sig = true;
                    }
                    prev = Some(*child);
                    continue;
                }
                ":" if emitted_sig => {
                    prev = Some(*child);
                    continue;
                }
                "type_annot" if emitted_sig => {
                    prev = Some(*child);
                    continue;
                }
                "func_block" => {
                    self.emit(" ");
                    self.format_func_block(*child);
                    return;
                }
                "nop" => {
                    let t = self.text(*child).to_string();
                    self.emit(&t);
                    self.nl();
                    return;
                }
                _ => {}
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
        self.nl();
    }

    fn format_func_sig(&mut self, func_node: Node) {
        let param_nodes = self.collect_func_param_nodes(func_node);
        // func_param_group → render via render_param_group (handles its own comment extras).
        // Everything else (comment extras at the func_params level) → verbatim source text.
        let rendered: Vec<String> = param_nodes
            .iter()
            .map(|n| {
                if n.kind() == "func_param_group" {
                    self.render_param_group(*n)
                } else {
                    self.text(*n).to_string()
                }
            })
            .collect();

        let ret_str = {
            let children: Vec<Node> = {
                let mut c = func_node.walk();
                func_node.children(&mut c).collect()
            };
            let mut past_params = false;
            let mut past_colon = false;
            let mut result = String::new();
            for child in &children {
                if child.kind() == "func_params" {
                    past_params = true;
                }
                if past_params && child.kind() == ":" && !child.is_missing() {
                    past_colon = true;
                }
                if past_colon && child.kind() == "type_annot" {
                    let colon = if self.compact_colon { ": " } else { " : " };
                    result = format!("{}{}", colon, self.text(*child));
                    break;
                }
            }
            result
        };

        // Build the inline string exhaustively. The comma is deferred: instead of
        // appending it right after a group, we set a pending flag and flush it just
        // before the *next* group. This keeps comment extras that trail a group (e.g.
        // `a : bool /*note*/,`) on the correct side of the comma.
        let inline = {
            let mut s = String::from("(");
            let mut needs_space = false;
            let mut pending_comma = false;
            for (i, r) in rendered.iter().enumerate() {
                let is_group = param_nodes[i].kind() == "func_param_group";
                if is_group && pending_comma {
                    s.push(',');
                    pending_comma = false;
                }
                if needs_space {
                    s.push(' ');
                }
                s.push_str(r);
                if is_group {
                    let has_later_group = param_nodes[i + 1..]
                        .iter()
                        .any(|n| n.kind() == "func_param_group");
                    if has_later_group {
                        pending_comma = true;
                    }
                }
                needs_space = true;
            }
            s.push(')');
            s
        };

        let group_count = param_nodes
            .iter()
            .filter(|n| n.kind() == "func_param_group")
            .count();
        let fits = group_count == 0
            || self.current_line_len() + inline.len() + ret_str.len() <= self.line_limit;

        if fits {
            self.emit(&inline);
            self.emit(&ret_str);
        } else {
            self.emit("(\n");
            self.level += 1;
            let mut emitted_groups = 0;
            for (node, r) in param_nodes.iter().zip(rendered.iter()) {
                self.emit_indent();
                self.emit(r);
                if node.kind() == "func_param_group" {
                    emitted_groups += 1;
                    if emitted_groups < group_count {
                        self.emit(",");
                    }
                }
                self.nl();
            }
            self.level -= 1;
            self.emit_indent();
            self.emit(")");
            self.emit(&ret_str);
        }
    }

    // Returns ALL named children of func_params (func_param_group nodes AND
    // comment extras). Named children only — anonymous tokens like `(`, `,`, `)`
    // are excluded. This ensures no comment that appears inside the parameter
    // list is ever dropped when the signature is reprinted.
    fn collect_func_param_nodes<'t>(&self, func_node: Node<'t>) -> Vec<Node<'t>> {
        self.child_of_kind(func_node, "func_params")
            .map(|params| {
                let mut c = params.walk();
                params.named_children(&mut c).collect()
            })
            .unwrap_or_default()
    }

    fn render_param_group(&self, node: Node) -> String {
        self.render_node(node)
    }

    fn format_class_decl(&mut self, node: Node) {
        if let Some(ann) = self.child_of_kind(node, "annotation") {
            let t = self.text(ann).to_string();
            self.emit_indent();
            self.emit(&t);
            self.nl();
        }
        self.emit_indent();
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
        let mut prev: Option<Node> = None;
        for child in &children {
            if child.is_missing() {
                continue;
            }
            if child.kind() == "annotation" {
                continue;
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

    fn format_state_decl(&mut self, node: Node) {
        self.emit_indent();
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
        let mut prev: Option<Node> = None;
        for child in &children {
            if child.is_missing() {
                continue;
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

    fn format_enum_decl(&mut self, node: Node) {
        self.emit_indent();
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
        let mut prev: Option<Node> = None;
        for child in &children {
            if child.is_missing() {
                continue;
            }
            if child.kind() == "enum_def" {
                self.emit(" ");
                self.format_enum_def(*child);
                return;
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
        self.nl();
    }

    fn format_enum_def(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
        // Exhaustive: all named children — enum_decl_variant AND comment extras.
        // Anonymous tokens ({, ,, }) are excluded by is_named() and handled directly.
        let members: Vec<Node> = children
            .iter()
            .filter(|n| n.is_named())
            .cloned()
            .collect();
        let open = children.iter().find(|n| n.kind() == "{");
        let close = children.iter().rfind(|n| n.kind() == "}");

        if let Some(o) = open {
            if !o.is_missing() {
                self.emit_verbatim(*o);
            }
        }
        if members.is_empty() {
            if let Some(cl) = close {
                if !cl.is_missing() {
                    self.emit_verbatim(*cl);
                }
            }
            self.nl();
            return;
        }
        self.nl();
        self.level += 1;
        let variant_count = members
            .iter()
            .filter(|n| n.kind() == "enum_decl_variant")
            .count();
        let mut emitted_variants = 0;
        for member in &members {
            self.emit_indent();
            if member.kind() == "enum_decl_variant" {
                self.format_children(*member);
                emitted_variants += 1;
                if emitted_variants < variant_count {
                    self.emit(",");
                }
            } else {
                self.emit_verbatim(*member);
            }
            self.nl();
        }
        self.level -= 1;
        self.emit_indent();
        if let Some(cl) = close {
            if !cl.is_missing() {
                self.emit_verbatim(*cl);
            }
        }
        self.nl();
    }

    fn format_class_def(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
        let members: Vec<Node> = children
            .iter()
            .filter(|n| n.is_named() && n.kind() != "nop")
            .cloned()
            .collect();
        let open = children.iter().find(|n| n.kind() == "{");
        let close = children.iter().rfind(|n| n.kind() == "}");

        if let Some(o) = open {
            if !o.is_missing() {
                let t = self.text(*o).to_string();
                self.emit(&t);
            }
        }
        if members.is_empty() {
            if let Some(cl) = close {
                if !cl.is_missing() {
                    let t = self.text(*cl).to_string();
                    self.emit(&t);
                }
            }
            self.nl();
            return;
        }
        self.nl();
        self.level += 1;

        let open_row = node.start_position().row;
        let mut prev_end_row: Option<usize> = None;
        let mut prev_was_comment = false;

        for member in &members {
            let child_row = member.start_position().row;
            let source_gap = match prev_end_row {
                Some(prev) => child_row.saturating_sub(prev),
                None => child_row.saturating_sub(open_row),
            };
            let is_callable = matches!(member.kind(), "func_decl" | "event_decl");
            let want_blank =
                source_gap >= 2 || (is_callable && prev_end_row.is_some() && !prev_was_comment);
            if want_blank {
                self.nl();
            }
            prev_was_comment = member.kind() == "comment";
            self.format_class_member(*member);
            prev_end_row = Some(member.end_position().row);
        }

        self.level -= 1;
        self.emit_indent();
        if let Some(cl) = close {
            if !cl.is_missing() {
                let t = self.text(*cl).to_string();
                self.emit(&t);
            }
        }
        self.nl();
    }

    fn format_class_member(&mut self, node: Node) {
        let has_comment_child = {
            let mut wc = node.walk();
            let result = node.children(&mut wc).any(|n| n.kind() == "comment");
            result
        };
        if node.is_error() || has_comment_child {
            let t = self.text(node).trim().to_string();
            self.emit_indent();
            self.emit(&t);
            self.nl();
            return;
        }
        match node.kind() {
            "func_decl" | "event_decl" => self.format_func_decl(node),
            "comment" => {
                let t = self.text(node).to_string();
                self.emit_indent();
                self.emit(&t);
                self.nl();
            }
            "member_default_val_block" => self.format_defaults_block(node),
            _ => {
                self.emit_indent();
                self.format_children(node);
                self.nl();
            }
        }
    }

    fn format_defaults_block(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
        // Exhaustive: all named children — member_default_val_block_assign AND
        // comment extras. The `defaults` keyword and {/} braces are anonymous
        // tokens and are excluded by is_named(), then handled directly below.
        let members: Vec<Node> = children
            .iter()
            .filter(|n| n.is_named())
            .cloned()
            .collect();
        let open = children.iter().find(|n| n.kind() == "{");
        let close = children.iter().rfind(|n| n.kind() == "}");

        self.emit_indent();
        if let Some(kw) = children.iter().find(|n| n.kind() == "defaults") {
            if !kw.is_missing() {
                let t = self.text(*kw).to_string();
                self.emit(&t);
            }
        }
        self.emit(" ");

        if let Some(o) = open {
            if !o.is_missing() {
                let t = self.text(*o).to_string();
                self.emit(&t);
            }
        }
        if members.is_empty() {
            if let Some(cl) = close {
                if !cl.is_missing() {
                    let t = self.text(*cl).to_string();
                    self.emit(&t);
                }
            }
            self.nl();
            return;
        }
        self.nl();
        self.level += 1;
        for member in &members {
            self.emit_indent();
            if member.kind() == "member_default_val_block_assign" {
                self.format_children(*member);
            } else {
                self.emit_verbatim(*member);
            }
            self.nl();
        }
        self.level -= 1;
        self.emit_indent();
        if let Some(cl) = close {
            if !cl.is_missing() {
                let t = self.text(*cl).to_string();
                self.emit(&t);
            }
        }
        self.nl();
    }

    // ---- Function body ----

    fn format_func_block(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
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
            self.nl();
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
        self.nl();
    }

    fn format_func_block_no_nl(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut c = node.walk();
            node.children(&mut c).collect()
        };
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
        if node.is_error() {
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

    fn format_if_stmt(&mut self, node: Node) {
        let force_block = self.if_chain_needs_block(node);
        self.format_if_stmt_emit(node, force_block);
    }

    fn format_if_stmt_emit(&mut self, node: Node, force_block: bool) {
        let cond = node.child_by_field_name("cond");
        let body = node.child_by_field_name("body");
        let else_body = node.child_by_field_name("else");

        let cond_text = cond
            .map(|c| normalize_expr(self.text(c)))
            .unwrap_or_default();
        let indent = self.level * self.indent_unit.len();
        let cond_line = indent + 4 + cond_text.len() + 2;

        if cond_line > self.line_limit {
            self.emit_indent();
            self.emit("if (\n");
            self.level += 1;
            for (fragment, op) in split_binary_condition(&cond_text) {
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
            self.emit(&cond_text);
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
                let line =
                    indent + 4 + normalize_expr(self.text(cond)).len() + 2 + self.text(body).len();
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
                            + 9
                            + normalize_expr(self.text(ec)).len()
                            + 2
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

    fn format_loop_stmt(&mut self, node: Node) {
        match node.kind() {
            "while_stmt" => {
                self.emit_indent();
                self.emit("while (");
                if let Some(cond) = node.child_by_field_name("cond") {
                    let t = normalize_expr(self.text(cond));
                    self.emit(&t);
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
                        self.format_func_block_no_nl(b);
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
                    let t = normalize_expr(self.text(cond));
                    self.emit(&t);
                }
                self.emit(")\n");
            }
            "for_stmt" => {
                self.emit_indent();
                self.emit("for (");
                if let Some(init) = node.child_by_field_name("init") {
                    let t = normalize_expr(self.text(init));
                    self.emit(&t);
                }
                self.emit("; ");
                if let Some(cond) = node.child_by_field_name("cond") {
                    let t = normalize_expr(self.text(cond));
                    self.emit(&t);
                }
                self.emit("; ");
                if let Some(iter) = node.child_by_field_name("iter") {
                    let t = normalize_expr(self.text(iter));
                    self.emit(&t);
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

    fn format_switch_stmt(&mut self, node: Node) {
        self.emit_indent();
        self.emit("switch (");
        if let Some(cond) = node.child_by_field_name("cond") {
            let t = normalize_expr(self.text(cond));
            self.emit(&t);
        }
        self.emit(") {\n");
        self.level += 1;
        if let Some(block) = self.child_of_kind(node, "switch_block") {
            let children: Vec<Node> = {
                let mut c = block.walk();
                block.children(&mut c).collect()
            };
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

    fn format_expr_stmt(&mut self, node: Node) {
        self.emit_indent();
        let expr = {
            let mut c = node.walk();
            let result = node.named_children(&mut c).next();
            result
        };
        if let Some(e) = expr {
            let normalized = normalize_expr(self.text(e));
            let indent = self.level * self.indent_unit.len();
            if indent + normalized.len() + 1 > self.line_limit {
                if let Some((prefix, args)) = try_split_call_args(&normalized) {
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
            self.emit(&normalized);
        }
        let semi = self.child_of_kind(node, ";");
        if semi.map(|n| !n.is_missing()).unwrap_or(false) {
            self.emit(";");
        }
        self.nl();
    }
}

#[cfg(test)]
mod tests {
    use crate::document::parse_document;

    fn fmt(source: &str) -> String {
        let doc = parse_document(source).expect("should parse");
        super::format_document(doc.tree.root_node(), &doc.source, 4, false, 100, false)
    }

    fn fmt_compact_colon(source: &str) -> String {
        let doc = parse_document(source).expect("should parse");
        super::format_document(doc.tree.root_node(), &doc.source, 4, false, 100, true)
    }

    fn fmt_limit(source: &str, line_limit: u32) -> String {
        let doc = parse_document(source).expect("should parse");
        super::format_document(
            doc.tree.root_node(),
            &doc.source,
            4,
            false,
            line_limit,
            false,
        )
    }

    #[test]
    fn error_recovery_formats_valid_stmts_around_invalid() {
        // var b has extra whitespace but is valid; var a is invalid (missing type annotation)
        let input = "function Test() {\n             var    b  : int;\n    var  a;\n}";
        let output = fmt(input);
        assert!(
            output.contains("var b : int;"),
            "valid stmt should be formatted, got:\n{output}"
        );
        assert!(
            output.contains("var  a;"),
            "invalid stmt should be preserved verbatim including semicolon, got:\n{output}"
        );
    }

    #[test]
    fn compact_colon_local_var() {
        let input = "function F() { var x : int; }";
        let output = fmt_compact_colon(input);
        assert!(output.contains("var x: int;"), "got:\n{output}");
    }

    #[test]
    fn compact_colon_member_var() {
        let input = "class C { var x : int; }";
        let output = fmt_compact_colon(input);
        assert!(output.contains("var x: int;"), "got:\n{output}");
    }

    #[test]
    fn compact_colon_func_param() {
        let input = "function F(x : int, y : bool) {}";
        let output = fmt_compact_colon(input);
        assert!(output.contains("(x: int, y: bool)"), "got:\n{output}");
    }

    #[test]
    fn compact_colon_return_type() {
        let input = "function F() : bool { return true; }";
        let output = fmt_compact_colon(input);
        assert!(output.contains("function F(): bool"), "got:\n{output}");
    }

    #[test]
    fn default_colon_style_unchanged() {
        let output = fmt("function F(x : int) : bool { var y : int; return true; }");
        assert!(output.contains("(x : int)"), "got:\n{output}");
        assert!(output.contains(") : bool"), "got:\n{output}");
        assert!(output.contains("var y : int;"), "got:\n{output}");
    }

    #[test]
    fn formats_simple_function() {
        let input = "function Foo(x:int):bool{return true;}";
        let output = fmt(input);
        assert!(output.contains("function Foo(x : int) : bool {"));
        assert!(output.contains("    return true;"));
        assert!(output.contains('}'));
    }

    #[test]
    fn formats_if_else() {
        let input = "function F() { if(x){a();} else{b();} }";
        let output = fmt(input);
        assert!(output.contains("if (x) {"), "got:\n{output}");
        assert!(
            output.contains("}\n    else {"),
            "else should be on new line, got:\n{output}"
        );
    }

    #[test]
    fn preserves_comment_only_class_body() {
        let input = "class C extends CPlayer {\n    // A comment\n}";
        let output = fmt(input);
        assert!(
            output.contains("// A comment"),
            "comment-only class body should not be collapsed to {{}}, got:\n{output}"
        );
    }

    #[test]
    fn formats_class_with_method() {
        let input = "class C extends B { var x : int; function M() {} }";
        let output = fmt(input);
        assert!(output.contains("class C extends B {"));
        assert!(output.contains("    var x : int;"));
        assert!(output.contains("    function M() {}"));
    }

    #[test]
    fn formats_enum() {
        let input = "enum EKind { A, B = 1, C = 2 }";
        let output = fmt(input);
        assert!(output.contains("enum EKind {"));
        assert!(output.contains("    A,"));
        assert!(output.contains("    B = 1,"));
    }

    #[test]
    fn formats_empty_state() {
        let input = "state Idle in Owner {}";
        let output = fmt(input);
        assert!(output.contains("state Idle in Owner {}"));
    }

    #[test]
    fn formats_for_loop() {
        let input = "function F() { for(i=0;i<10;i+=1){a();} }";
        let output = fmt(input);
        assert!(output.contains("for (i=0; i<10; i+=1) {"));
    }

    #[test]
    fn normalizes_expr_whitespace() {
        let input = "function F() { var x : int = SomeObj  .Method   (  ); }";
        let output = fmt(input);
        assert!(
            output.contains("var x : int = SomeObj.Method();"),
            "got:\n{output}"
        );
    }

    #[test]
    fn normalizes_extra_spaces_in_call() {
        let input = "function F() { SomeFunc   (  a,   b  ); }";
        let output = fmt(input);
        assert!(output.contains("SomeFunc(a, b);"), "got:\n{output}");
    }

    #[test]
    fn inline_single_stmt_if() {
        let input = "function F() { if (x)\n    return; }";
        let output = fmt(input);
        assert!(
            output.contains("if (x) return;"),
            "single-stmt if body should be on same line, got:\n{output}"
        );
    }

    #[test]
    fn inline_single_stmt_if_else() {
        let input = "function F() { if (x)\n    return;\nelse\n    break; }";
        let output = fmt(input);
        assert!(output.contains("if (x) return;"), "got:\n{output}");
        assert!(
            output.contains("    else break;"),
            "else should be on new line, got:\n{output}"
        );
    }

    #[test]
    fn block_if_else_else_on_new_line() {
        let input = "function F() { if(x){a();} else{b();} }";
        let output = fmt(input);
        assert!(output.contains("if (x) {"), "got:\n{output}");
        assert!(
            output.contains("}\n    else {"),
            "else should be on new line, got:\n{output}"
        );
    }

    #[test]
    fn long_line_forces_block_form() {
        // Condition alone pushes the inline line over 100 chars
        let long_cond =
            "veryLongVariableName.IsWayTooLong.SeriouslyNeedsToBeSmaller().DoesntFitWell > 1";
        let input = format!("function F() {{\n    if (expr) DoThing();\n    else if ({long_cond})\n        return;\n    else\n        Log(\"Something\");\n}}");
        let output = fmt(&input);
        assert!(
            output.contains("if (expr) {"),
            "should wrap short if to block when chain is long, got:\n{output}"
        );
        assert!(
            output.contains("else if (") && output.contains(") {"),
            "else-if should use block, got:\n{output}"
        );
        assert!(
            output.contains("else {"),
            "final else should use block, got:\n{output}"
        );
    }

    #[test]
    fn preserves_single_blank_line_in_body() {
        let input = "function F() {\n    a();\n\n    b();\n}";
        let output = fmt(input);
        assert!(
            output.contains("a();\n\n    b();"),
            "blank line should be preserved"
        );
    }

    #[test]
    fn collapses_multiple_blank_lines_to_one() {
        let input = "function F() {\n    a();\n\n\n    b();\n}";
        let output = fmt(input);
        assert!(
            output.contains("a();\n\n    b();"),
            "multiple blank lines should collapse to one"
        );
        assert!(
            !output.contains("a();\n\n\n"),
            "should not have two consecutive blank lines"
        );
    }

    #[test]
    fn idempotent_on_valid_fixture() {
        let source = include_str!("../tests/fixtures/valid/basic_function.ws");
        let first = fmt(source);
        let second = fmt(&first);
        assert_eq!(first, second, "formatter should be idempotent");
    }

    #[test]
    fn blank_line_between_class_fields_preserved() {
        let input = "class C extends B {\n    var a : int;\n\n    var b : int;\n}";
        let output = fmt(input);
        assert!(
            output.contains("var a : int;\n\n    var b : int;"),
            "blank line between fields should be preserved, got:\n{output}"
        );
    }

    #[test]
    fn blank_line_at_class_start_preserved() {
        let input = "class C extends B {\n\n    var a : int;\n}";
        let output = fmt(input);
        assert!(
            output.contains("{\n\n    var a : int;"),
            "leading blank line inside class body should be preserved, got:\n{output}"
        );
    }

    #[test]
    fn multiple_blank_lines_in_class_condensed_to_one() {
        let input = "class C extends B {\n    var a : int;\n\n\n    var b : int;\n}";
        let output = fmt(input);
        assert!(
            output.contains("var a : int;\n\n    var b : int;"),
            "multiple blank lines should collapse to one, got:\n{output}"
        );
        assert!(
            !output.contains("var a : int;\n\n\n"),
            "should not have two consecutive blank lines, got:\n{output}"
        );
    }

    #[test]
    fn no_blank_line_between_adjacent_class_fields() {
        let input = "class C extends B {\n    var a : int;\n    var b : int;\n}";
        let output = fmt(input);
        assert!(
            !output.contains("var a : int;\n\n"),
            "adjacent fields with no blank line in source should not gain one, got:\n{output}"
        );
    }

    #[test]
    fn long_func_signature_splits_params() {
        // "function LongFuncName(paramOne : int, paramTwo : bool, paramThree : string) : bool {"
        // is 88 chars — force split with a 60-char limit
        let src = "function LongFuncName(paramOne:int,paramTwo:bool,paramThree:string):bool{}";
        let out = fmt_limit(src, 60);
        assert!(
            out.contains("function LongFuncName(\n"),
            "opening paren should be followed by newline, got:\n{out}"
        );
        assert!(out.contains("    paramOne : int,\n"), "got:\n{out}");
        assert!(out.contains("    paramTwo : bool,\n"), "got:\n{out}");
        assert!(out.contains("    paramThree : string\n"), "got:\n{out}");
        assert!(
            out.contains(") : bool {"),
            "closing paren + return type, got:\n{out}"
        );
    }

    #[test]
    fn short_func_signature_stays_inline() {
        let src = "function Short(a:int):bool{return true;}";
        let out = fmt_limit(src, 100);
        assert!(
            !out.contains("(\n"),
            "short signature should not split, got:\n{out}"
        );
        assert!(
            out.contains("function Short(a : int) : bool {"),
            "got:\n{out}"
        );
    }

    #[test]
    fn no_param_func_never_splits() {
        let src = "function NoParams():bool{return true;}";
        // Tiny limit: zero-param list should never be split regardless of limit
        let out = fmt_limit(src, 10);
        assert!(
            !out.contains("(\n"),
            "no-param func should never split, got:\n{out}"
        );
    }

    #[test]
    fn trailing_comment_on_error_member_var_preserved() {
        // Missing ";" makes member_var_decl an error node;
        // the trailing comment must survive and no spurious ";" must be added.
        let input = "class C {\n    var x : int // trailing comment\n}";
        let output = fmt(input);
        assert!(
            output.contains("// trailing comment"),
            "trailing comment on error member_var_decl must be preserved, got:\n{output}"
        );
        assert!(
            !output.contains("// trailing comment;"),
            "spurious semicolon must not be appended after the comment, got:\n{output}"
        );
    }

    #[test]
    fn trailing_comment_on_member_default_val_preserved() {
        // member_default_val is structurally valid (real ";" on next line),
        // but carries an inline comment that must not be dropped.
        let input = "class C {\n    var x : int;\n    default x = OT_None // keep me\n    ;\n}";
        let output = fmt(input);
        assert!(
            output.contains("// keep me"),
            "trailing comment on member_default_val must be preserved, got:\n{output}"
        );
    }

    #[test]
    fn class_method_params_wrapped_when_body_has_error() {
        // A func_decl whose body contains an error (missing semicolon on a statement)
        // must still have its long parameter list wrapped — the error inside the body
        // must not cause the entire function to be emitted verbatim.
        let input = concat!(
            "class C {\n",
            "    function SomeLongMethodName(firstParam : SomeLongType, secondParam : AnotherLongType, thirdParam : YetAnotherType) : bool {\n",
            "        SomeCall() // missing semicolon\n",
            "    }\n",
            "}"
        );
        let output = fmt(input);
        assert!(
            output.contains("(\n"),
            "long class method params must be split to multiple lines even when body has error, got:\n{output}"
        );
    }

    #[test]
    fn member_default_val_with_ident_value_preserved() {
        // OT_None is an ident-typed node; the "= OT_None" must not be dropped
        let input = "class C extends B {\n    default isPotato = OT_None;\n}";
        let output = fmt(input);
        assert!(
            output.contains("default isPotato = OT_None;"),
            "default value that is an identifier must be preserved, got:\n{output}"
        );
    }

    #[test]
    fn local_var_init_with_ident_value_preserved() {
        // Initializer is an identifier (enum value) — must not be dropped
        let input = "function F() { var x : EOrientationTarget = OT_None; }";
        let output = fmt(input);
        assert!(
            output.contains("var x : EOrientationTarget = OT_None;"),
            "var initializer that is an identifier must be preserved, got:\n{output}"
        );
    }

    #[test]
    fn long_call_stmt_splits_args() {
        // "    SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10);"
        // = 4+67+1 = 72 chars > 60
        let src =
            "function F() { SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10); }";
        let out = fmt_limit(src, 60);
        assert!(
            out.contains("SetupEnemiesCollection(\n"),
            "long call should split, got:\n{out}"
        );
        assert!(out.contains("enemyCollectionDist,\n"), "got:\n{out}");
        assert!(out.contains("findMoveTargetDist,\n"), "got:\n{out}");
        assert!(out.contains(");\n"), "got:\n{out}");
    }

    #[test]
    fn short_call_stmt_stays_inline() {
        let src = "function F() { Foo(a, b); }";
        let out = fmt(src);
        assert!(
            !out.contains("Foo(\n"),
            "short call should stay inline, got:\n{out}"
        );
        assert!(out.contains("Foo(a, b);"), "got:\n{out}");
    }

    #[test]
    fn split_call_stmt_is_idempotent() {
        let src =
            "function F() { SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10); }";
        let first = fmt_limit(src, 60);
        let second = fmt_limit(&first, 60);
        assert_eq!(first, second, "split call stmt should be idempotent");
    }

    #[test]
    fn comment_before_param_preserved() {
        let input = "function lossy(/* comment */ param1 : bool) {}";
        let output = fmt(input);
        assert!(
            output.contains("/* comment */"),
            "leading comment inside param list must be preserved, got:\n{output}"
        );
        assert!(
            output.contains("param1"),
            "param name must still be present, got:\n{output}"
        );
    }

    #[test]
    fn comment_between_params_preserved() {
        let input = "function F(a : int, /* mid */ b : bool) {}";
        let output = fmt(input);
        assert!(
            output.contains("/* mid */"),
            "inter-param comment must be preserved, got:\n{output}"
        );
    }

    #[test]
    fn comment_in_enum_body_preserved() {
        let input = "enum EKind {\n    // a comment\n    A,\n    B\n}";
        let output = fmt(input);
        assert!(
            output.contains("// a comment"),
            "comment inside enum body must be preserved, got:\n{output}"
        );
    }

    #[test]
    fn comment_in_defaults_block_preserved() {
        let input =
            "class C {\n    defaults {\n        // a comment\n        x = 1;\n    }\n}";
        let output = fmt(input);
        assert!(
            output.contains("// a comment"),
            "comment inside defaults block must be preserved, got:\n{output}"
        );
    }

    #[test]
    fn comment_trailing_param_preserved() {
        let input = "function F(a : int /* trailing */) {}";
        let output = fmt(input);
        assert!(
            output.contains("/* trailing */"),
            "trailing comment after last param must be preserved, got:\n{output}"
        );
    }

    #[test]
    fn comment_between_params_comma_position() {
        // Comma must land AFTER trailing comments on a param group, not before them.
        // Old bug: "(a : bool, /*b*/ /*a*/ i : int)" — comma before both comments.
        let input = "function F(a : bool /*b*/,/*a*/ i : int) {}";
        let output = fmt(input);
        assert!(output.contains("/*b*/"), "first comment dropped, got:\n{output}");
        assert!(output.contains("/*a*/"), "second comment dropped, got:\n{output}");
        assert!(
            !output.contains("bool,"),
            "comma must not appear immediately after type (before trailing comments), got:\n{output}"
        );
    }

    #[test]
    fn split_signature_is_idempotent() {
        let src = "function LongFuncName(paramOne:int,paramTwo:bool,paramThree:string):bool{}";
        let first = fmt_limit(src, 60);
        let second = fmt_limit(&first, 60);
        assert_eq!(first, second, "split-param formatting should be idempotent");
    }

    #[test]
    fn long_if_condition_splits_onto_own_lines() {
        // "    if (alpha || beta || gamma)" = 4+4+22+1 = 31 chars; limit 30 forces split
        let src = "function F() { if (alpha || beta || gamma) return; }";
        let out = fmt_limit(src, 30);
        assert!(
            out.contains("if (\n"),
            "condition should open on its own line, got:\n{out}"
        );
        assert!(
            out.contains("alpha ||\n"),
            "each operand should be on its own line with op at end, got:\n{out}"
        );
        assert!(out.contains("beta ||\n"), "got:\n{out}");
        assert!(
            out.contains("gamma\n"),
            "last operand has no trailing op, got:\n{out}"
        );
        assert!(
            out.contains(") {\n"),
            "multiline condition must force block body, got:\n{out}"
        );
        assert!(out.contains("return;"), "body must be emitted, got:\n{out}");
    }

    #[test]
    fn short_if_condition_not_split() {
        let src = "function F() { if (x > 0) return; }";
        let out = fmt(src); // default 100-char limit
        assert!(
            !out.contains("if (\n"),
            "short condition should stay inline, got:\n{out}"
        );
    }

    #[test]
    fn long_if_condition_with_and_operators() {
        // &&-only condition also splits correctly
        let src = "function F() { if (conditionAlpha && conditionBeta && conditionGamma) return; }";
        let out = fmt_limit(src, 40);
        // "    if (conditionAlpha && conditionBeta && conditionGamma)" = 4+4+51+1 = 60 > 40
        assert!(out.contains("if (\n"), "got:\n{out}");
        assert!(out.contains("conditionAlpha &&\n"), "got:\n{out}");
    }

    #[test]
    fn multiline_if_condition_is_idempotent() {
        let src = "function F() { if (alpha || beta || gamma) return; }";
        let first = fmt_limit(src, 30);
        let second = fmt_limit(&first, 30);
        assert_eq!(
            first, second,
            "multiline if condition formatting should be idempotent"
        );
    }
}
