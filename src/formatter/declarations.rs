use tree_sitter::Node;

use super::{child_nodes, is_alignable_field, is_bodiless_callable, Formatter};

impl<'a> Formatter<'a> {
    // ---- Top level ----

    pub(super) fn format_script(&mut self, node: Node) {
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

    fn annotation_same_line_in_source(&self, node: Node, ann: Node) -> bool {
        let ann_row = ann.end_position().row;
        let mut c = node.walk();
        for child in node.children(&mut c) {
            if child.is_missing() || child.kind() == "annotation" {
                continue;
            }
            return child.start_position().row == ann_row;
        }
        false
    }

    fn is_add_field_annotation(&self, ann: Node) -> bool {
        self.child_of_kind(ann, "annotation_ident")
            .is_some_and(|ident| self.text(ident).trim_start_matches('@') == "addField")
    }

    fn emit_annotation(&mut self, ann: Node) {
        let ann_text = self.render_node(ann);
        self.emit_indent();
        self.emit(&ann_text);
        self.nl();
    }

    fn emit_add_field_annotation(&mut self, node: Node, ann: Node) -> bool {
        let same_line = match self.annotation_placement {
            super::AnnotationPlacement::SameLine => true,
            super::AnnotationPlacement::OwnLine => false,
            super::AnnotationPlacement::Preserve => self.annotation_same_line_in_source(node, ann),
        };
        let ann_text = self.render_node(ann);
        self.emit_indent();
        self.emit(&ann_text);
        if same_line {
            self.emit(" ");
            true
        } else {
            self.nl();
            false
        }
    }

    pub(super) fn format_member_var_decl(&mut self, node: Node, colon_align_col: Option<usize>) {
        let same_line = if let Some(ann) = self.child_of_kind(node, "annotation") {
            if self.is_add_field_annotation(ann) {
                self.emit_add_field_annotation(node, ann)
            } else {
                self.emit_annotation(ann);
                false
            }
        } else {
            false
        };
        if !same_line {
            self.emit_indent();
        }
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

    pub(super) fn format_func_decl(&mut self, node: Node) {
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

    pub(super) fn format_class_decl(&mut self, node: Node) {
        self.emit_indent();
        self.format_children(node);
    }

    pub(super) fn format_enum_decl(&mut self, node: Node) {
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
        let member_count = members
            .iter()
            .filter(|n| n.kind() == "enum_decl_variant")
            .count();
        let mut emitted_members = 0;
        for member in &members {
            self.emit_indent();
            if member.kind() == "enum_decl_variant" {
                self.format_children(*member);
                emitted_members += 1;
                if emitted_members < member_count {
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

    pub(super) fn format_class_def(&mut self, node: Node) {
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
}
