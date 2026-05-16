use tree_sitter::Node;

fn render_expr(node: Node, source: &str) -> String {
    Formatter {
        source,
        indent_unit: String::new(),
        level: 0,
        out: String::new(),
        suppress_next_indent: false,
        line_limit: usize::MAX,
        compact_colon: false,
        align_member_colons: false,
        colon_align_col: None,
    }
    .render_node(node)
}

fn collect_bool_parts(node: Node, source: &str, parts: &mut Vec<(String, Option<&'static str>)>) {
    if node.kind() == "binary_op_expr" {
        if let Some(op_node) = node.child_by_field_name("op") {
            let op_str: Option<&'static str> = match op_node.kind() {
                "binary_op_or" => Some("||"),
                "binary_op_and" => Some("&&"),
                _ => None,
            };
            if let Some(op) = op_str {
                if let (Some(left), Some(right)) = (
                    node.child_by_field_name("left"),
                    node.child_by_field_name("right"),
                ) {
                    collect_bool_parts(left, source, parts);
                    if let Some(last) = parts.last_mut() {
                        last.1 = Some(op);
                    }
                    collect_bool_parts(right, source, parts);
                    return;
                }
            }
        }
    }
    parts.push((render_expr(node, source), None));
}

fn split_binary_condition(node: Node, source: &str) -> Vec<(String, Option<&'static str>)> {
    let mut parts = Vec::new();
    collect_bool_parts(node, source, &mut parts);
    parts
}

fn try_split_call_args(node: Node, source: &str) -> Option<(String, Vec<String>)> {
    if node.kind() != "func_call_expr" {
        return None;
    }
    let func = node.child_by_field_name("func")?;
    let args_node = node.child_by_field_name("args")?;
    let args: Vec<String> = {
        let mut cursor = args_node.walk();
        args_node
            .children(&mut cursor)
            .filter(|c| c.kind() != ",")
            .map(|c| render_expr(c, source))
            .collect()
    };
    if args.len() <= 1 {
        return None;
    }
    let prefix = render_expr(func, source);
    Some((prefix, args))
}

fn child_nodes(node: Node) -> Vec<Node> {
    let mut c = node.walk();
    node.children(&mut c).collect()
}

fn named_child_nodes(node: Node) -> Vec<Node> {
    let mut c = node.walk();
    node.named_children(&mut c).collect()
}

fn is_alignable_field(node: Node) -> bool {
    if node.kind() != "member_var_decl" || node.is_error() {
        return false;
    }
    let mut c = node.walk();
    let has_comment = node.children(&mut c).any(|n| n.kind() == "comment");
    !has_comment
}

fn is_bodiless_callable(node: Node) -> bool {
    if !matches!(node.kind(), "func_decl" | "event_decl") {
        return false;
    }
    let mut c = node.walk();
    let has_block = node.children(&mut c).any(|n| n.kind() == "func_block");
    !has_block
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

/// Renders the parameter list and return type of a callable declaration node as a
/// clean, normalised string — comments stripped, whitespace canonical.
/// Returns `None` if the node has no `func_params` child.
pub fn render_callable_signature(node: Node, source: &str) -> Option<String> {
    let f = Formatter {
        source,
        indent_unit: String::new(),
        level: 0,
        out: String::new(),
        suppress_next_indent: false,
        line_limit: usize::MAX,
        compact_colon: true,
        align_member_colons: false,
        colon_align_col: None,
    };
    f.render_sig(node)
}

pub fn format_document(
    root: Node,
    source: &str,
    tab_size: u32,
    use_tabs: bool,
    line_limit: u32,
    compact_colon: bool,
    align_member_colons: bool,
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
        align_member_colons,
        colon_align_col: None,
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
    align_member_colons: bool,
    colon_align_col: Option<usize>,
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
            "class_decl" | "struct_decl" | "state_decl" => self.format_class_decl(node),
            "enum_decl" => self.format_enum_decl(node),
            "member_var_decl" => self.format_member_var_decl(node, None),
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

    fn format_children(&mut self, node: Node) {
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

    // ---- Top level ----

    fn format_script(&mut self, node: Node) {
        let children: Vec<Node> = child_nodes(node)
            .into_iter()
            .filter(|n| n.is_named() && n.kind() != "nop")
            .collect();
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

    fn emit_annotation(&mut self, ann: Node) {
        let t = self.text(ann).to_string();
        self.emit_indent();
        self.emit(&t);
        self.nl();
    }

    fn format_member_var_decl(&mut self, node: Node, colon_align_col: Option<usize>) {
        if let Some(ann) = self.child_of_kind(node, "annotation") {
            self.emit_annotation(ann);
        }
        self.emit_indent();
        self.colon_align_col = colon_align_col;
        self.format_children(node);
        self.colon_align_col = None;
        self.nl();
    }

    fn member_var_pre_colon_width(&self, node: Node) -> usize {
        let children = child_nodes(node);
        let Some(colon) = children.iter().position(|c| c.kind() == ":") else {
            return 0;
        };
        let mut width = 0;
        let mut prev: Option<Node> = None;
        for child in &children[..colon] {
            if child.is_missing() || child.kind() == "annotation" {
                continue;
            }
            if let Some(p) = prev {
                if self.gap_between(p, *child, node.kind()) {
                    width += 1;
                }
            }
            width += self.render_node(*child).len();
            prev = Some(*child);
        }
        width
    }

    // Targets for colon-aligning runs of consecutive field declarations. A run is a
    // maximal sequence of plain `member_var_decl` members separated by at most one
    // newline (a blank line breaks the run). Single-member runs are left unaligned.
    fn member_colon_targets(&self, members: &[Node]) -> Vec<Option<usize>> {
        let mut targets = vec![None; members.len()];
        if !self.align_member_colons {
            return targets;
        }
        let indent_width = self.level * self.indent_unit.len();
        let mut i = 0;
        while i < members.len() {
            if !is_alignable_field(members[i]) {
                i += 1;
                continue;
            }
            let mut j = i;
            while j + 1 < members.len()
                && is_alignable_field(members[j + 1])
                && members[j + 1]
                    .start_position()
                    .row
                    .saturating_sub(members[j].end_position().row)
                    <= 1
            {
                j += 1;
            }
            if j > i {
                let width = (i..=j)
                    .map(|k| self.member_var_pre_colon_width(members[k]))
                    .max()
                    .unwrap_or(0);
                for target in targets.iter_mut().take(j + 1).skip(i) {
                    *target = Some(indent_width + width);
                }
            }
            i = j + 1;
        }
        targets
    }

    fn format_func_decl(&mut self, node: Node) {
        if let Some(ann) = self.child_of_kind(node, "annotation") {
            self.emit_annotation(ann);
        }
        self.emit_indent();

        let children = child_nodes(node);
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
        // Keep `,` tokens in the walk so comments land on the correct side.
        let inner_nodes: Vec<Node> = self
            .child_of_kind(func_node, "func_params")
            .map(|fp| {
                let mut c = fp.walk();
                fp.children(&mut c)
                    .filter(|n| !n.is_missing() && n.kind() != "(" && n.kind() != ")")
                    .collect()
            })
            .unwrap_or_default();
        let rendered: Vec<String> = inner_nodes
            .iter()
            .map(|n| {
                if n.kind() == "func_param_group" {
                    self.render_param_group(*n)
                } else {
                    self.text(*n).to_string()
                }
            })
            .collect();

        let ret_str = self.return_type_suffix(func_node);

        let inline = {
            let mut s = String::from("(");
            for (i, r) in rendered.iter().enumerate() {
                if i > 0 && inner_nodes[i].kind() != "," {
                    s.push(' ');
                }
                s.push_str(r);
            }
            s.push(')');
            s
        };

        let group_count = inner_nodes
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
            for (node, r) in inner_nodes.iter().zip(rendered.iter()) {
                if node.kind() == "," {
                    continue;
                }
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

    fn collect_func_param_nodes<'t>(&self, func_node: Node<'t>) -> Vec<Node<'t>> {
        self.child_of_kind(func_node, "func_params")
            .map(named_child_nodes)
            .unwrap_or_default()
    }

    fn render_param_group(&self, node: Node) -> String {
        self.render_node(node)
    }

    fn return_type_suffix(&self, func_node: Node) -> String {
        let mut c = func_node.walk();
        let mut past_params = false;
        let mut past_colon = false;
        for child in func_node.children(&mut c) {
            if child.kind() == "func_params" {
                past_params = true;
            }
            if past_params && child.kind() == ":" && !child.is_missing() {
                past_colon = true;
            }
            if past_colon && child.kind() == "type_annot" {
                let colon = if self.compact_colon { ": " } else { " : " };
                return format!("{}{}", colon, self.text(child));
            }
        }
        String::new()
    }

    fn render_sig(&self, func_node: Node) -> Option<String> {
        let param_groups: Vec<String> = self
            .collect_func_param_nodes(func_node)
            .into_iter()
            .filter(|n| n.kind() == "func_param_group")
            .map(|n| self.render_param_group(n))
            .collect();

        // Guard: no func_params means this isn't a callable we can render.
        self.child_of_kind(func_node, "func_params")?;

        let ret_str = self.return_type_suffix(func_node);

        Some(format!("({}){}", param_groups.join(", "), ret_str))
    }

    fn format_class_decl(&mut self, node: Node) {
        self.emit_indent();
        self.format_children(node);
    }

    fn format_enum_decl(&mut self, node: Node) {
        self.emit_indent();
        let children = child_nodes(node);
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
        let children = child_nodes(node);
        // Exhaustive: all named children — enum_decl_variant AND comment extras.
        // Anonymous tokens ({, ,, }) are excluded by is_named() and handled directly.
        let members: Vec<Node> = children.iter().filter(|n| n.is_named()).cloned().collect();
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
        let children = child_nodes(node);
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

        let colon_targets = self.member_colon_targets(&members);
        let open_row = node.start_position().row;
        let mut prev_end_row: Option<usize> = None;
        let mut prev_was_comment = false;
        let mut prev_member: Option<Node> = None;

        for (idx, member) in members.iter().enumerate() {
            let child_row = member.start_position().row;
            let source_gap = match prev_end_row {
                Some(prev) => child_row.saturating_sub(prev),
                None => child_row.saturating_sub(open_row),
            };
            let is_callable = matches!(member.kind(), "func_decl" | "event_decl");
            let both_bodiless = is_bodiless_callable(*member)
                && prev_member.map(is_bodiless_callable).unwrap_or(false);
            let want_blank = source_gap >= 2
                || (is_callable && prev_end_row.is_some() && !prev_was_comment && !both_bodiless);
            if want_blank {
                self.nl();
            }
            prev_was_comment = member.kind() == "comment";
            self.format_class_member(*member, colon_targets[idx]);
            prev_end_row = Some(member.end_position().row);
            prev_member = Some(*member);
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

    fn format_class_member(&mut self, node: Node, colon_align_col: Option<usize>) {
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
            "member_var_decl" => self.format_member_var_decl(node, colon_align_col),
            _ => {
                self.emit_indent();
                self.format_children(node);
                self.nl();
            }
        }
    }

    fn format_defaults_block(&mut self, node: Node) {
        let children = child_nodes(node);
        // Exhaustive: all named children — member_default_val_block_assign AND
        // comment extras. The `defaults` keyword and {/} braces are anonymous
        // tokens and are excluded by is_named(), then handled directly below.
        let members: Vec<Node> = children.iter().filter(|n| n.is_named()).cloned().collect();
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

        let indent = self.level * self.indent_unit.len();
        let cond_len = cond.map(|c| self.render_node(c).len()).unwrap_or(0);
        let cond_line = indent + 4 + cond_len + 2;

        if cond_line > self.line_limit {
            self.emit_indent();
            self.emit("if (\n");
            self.level += 1;
            let parts = if let Some(c) = cond {
                split_binary_condition(c, self.source)
            } else {
                vec![]
            };
            for (fragment, op) in parts {
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

    fn format_loop_stmt(&mut self, node: Node) {
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

    fn format_switch_stmt(&mut self, node: Node) {
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

    fn format_expr_stmt(&mut self, node: Node) {
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

#[cfg(test)]
mod tests;
