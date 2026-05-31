use tree_sitter::Node;

use super::{child_nodes, is_alignable_field, is_bodiless_callable, Formatter};

const DEFAULT_GAP: &str = "  ";

impl<'a> Formatter<'a> {
    // ---- Top level ----

    pub(super) fn format_script(&mut self, node: Node) {
        let children: Vec<Node> = child_nodes(node)
            .into_iter()
            .filter(|n| n.is_named() && n.kind() != "nop")
            .collect();
        let mut prev_end_row: Option<usize> = None;
        let mut prev_was_comment = false;
        let mut prev_node: Option<Node> = None;
        for child in &children {
            let child_row = child.start_position().row;
            let trailing = child.kind() == "comment" && self.is_trailing_comment(prev_node, *child);
            if let Some(prev) = prev_end_row {
                let source_gap = child_row.saturating_sub(prev);
                if !trailing && (source_gap >= 2 || !prev_was_comment) {
                    self.nl();
                }
            }
            prev_was_comment = child.kind() == "comment";
            if child.kind() == "comment" {
                self.flush_comments_before(child.end_byte());
            } else {
                self.format_node(*child);
            }
            prev_end_row = Some(child.end_position().row);
            prev_node = Some(*child);
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
        self.flush_comments_before(ann.start_byte());
        let ann_text = self.render_node(ann);
        self.emit_indent();
        self.emit(&ann_text);
        self.consume_comments_before(ann.end_byte());
        self.nl();
    }

    fn emit_add_field_annotation(&mut self, node: Node, ann: Node) -> bool {
        let placement = self.annotation_placement;
        let same_line = placement.resolve(|| self.annotation_same_line_in_source(node, ann));
        self.flush_comments_before(ann.start_byte());
        let ann_text = self.render_node(ann);
        self.emit_indent();
        self.emit(&ann_text);
        self.consume_comments_before(ann.end_byte());
        if same_line {
            self.emit(" ");
            true
        } else {
            self.nl();
            false
        }
    }

    fn first_ident_text(&self, node: Node) -> Option<&str> {
        self.child_of_kind(node, "ident").map(|n| self.text(n))
    }

    fn paired_member_default(&self, var_decl: Node, default_val: Node) -> bool {
        matches!(
            (
                self.first_ident_text(var_decl),
                self.first_ident_text(default_val)
            ),
            (Some(var_name), Some(default_name)) if var_name == default_name
        )
    }

    fn default_same_line_in_source(&self, var_decl: Node, default_val: Node) -> bool {
        var_decl.start_position().row == default_val.start_position().row
    }

    fn default_on_same_line(&self, var_decl: Node, default_val: Node) -> bool {
        if !self.paired_member_default(var_decl, default_val) {
            return false;
        }
        let placement = self.default_placement;
        placement.resolve(|| self.default_same_line_in_source(var_decl, default_val))
    }

    pub(super) fn format_member_var_decl(
        &mut self,
        node: Node,
        colon_align_col: Option<usize>,
        trailing_default: Option<(Node, Option<usize>)>,
    ) {
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
        if let Some((default_val, default_align_col)) = trailing_default {
            if let Some(col) = default_align_col {
                while self.current_line_len() < col {
                    self.emit(" ");
                }
            } else {
                self.emit(DEFAULT_GAP);
            }
            self.format_children(default_val);
        }
        self.nl();
    }

    fn rendered_width(&self, children: &[Node], parent_kind: &str) -> usize {
        let mut width = 0;
        let mut prev: Option<Node> = None;
        for child in children {
            if child.is_missing() || child.kind() == "annotation" {
                continue;
            }
            if let Some(p) = prev {
                if self.gap_between(p, *child, parent_kind) {
                    width += 1;
                }
            }
            width += self.render_node(*child).len();
            prev = Some(*child);
        }
        width
    }

    fn member_var_suffix_from_colon_width(&self, node: Node) -> usize {
        let children = child_nodes(node);
        let Some(colon_idx) = children.iter().position(|c| c.kind() == ":") else {
            return 0;
        };
        self.rendered_width(&children[colon_idx..], node.kind())
    }

    fn member_var_line_width_to_semicolon(
        &self,
        node: Node,
        colon_align_col: Option<usize>,
    ) -> usize {
        let indent_width = self.level * self.indent_unit.len();
        let suffix = self.member_var_suffix_from_colon_width(node);
        match colon_align_col {
            None => self.member_var_decl_width(node),
            Some(col) => col.saturating_sub(indent_width) + suffix,
        }
    }

    fn member_var_decl_width(&self, node: Node) -> usize {
        self.rendered_width(&child_nodes(node), node.kind())
    }

    fn member_var_pre_colon_width(&self, node: Node) -> usize {
        let children = child_nodes(node);
        let Some(colon) = children.iter().position(|c| c.kind() == ":") else {
            return 0;
        };
        self.rendered_width(&children[..colon], node.kind())
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
        let mut idx = 0;
        while idx < members.len() {
            if self.is_mergeable_default_pair(members, idx) {
                let run_end = self.same_line_default_pair_run_end(members, idx);
                let pair_count = (run_end - idx) / 2;
                if pair_count >= 2 {
                    let width = (0..pair_count)
                        .map(|k| self.member_var_pre_colon_width(members[idx + k * 2]))
                        .max()
                        .unwrap_or(0);
                    let col = indent_width + width;
                    for k in 0..pair_count {
                        targets[idx + k * 2] = Some(col);
                    }
                }
                idx = run_end;
            } else if is_alignable_field(members[idx]) {
                let mut j = idx;
                while j + 1 < members.len()
                    && is_alignable_field(members[j + 1])
                    && !self.is_mergeable_default_pair(members, j + 1)
                    && members[j + 1]
                        .start_position()
                        .row
                        .saturating_sub(members[j].end_position().row)
                        <= 1
                {
                    j += 1;
                }
                if j > idx {
                    let width = (idx..=j)
                        .map(|k| self.member_var_pre_colon_width(members[k]))
                        .max()
                        .unwrap_or(0);
                    for target in targets.iter_mut().take(j + 1).skip(idx) {
                        *target = Some(indent_width + width);
                    }
                }
                idx = j + 1;
            } else {
                idx += 1;
            }
        }
        targets
    }

    fn is_mergeable_default_pair(&self, members: &[Node], var_idx: usize) -> bool {
        let Some(default) = members.get(var_idx + 1) else {
            return false;
        };
        members[var_idx].kind() == "member_var_decl"
            && default.kind() == "member_default_val"
            && is_alignable_field(members[var_idx])
            && self.default_on_same_line(members[var_idx], *default)
    }

    fn same_line_default_pair_run_end(&self, members: &[Node], run_start: usize) -> usize {
        let mut next_var = run_start;
        while next_var + 1 < members.len() && self.is_mergeable_default_pair(members, next_var) {
            if next_var > run_start {
                let gap = members[next_var]
                    .start_position()
                    .row
                    .saturating_sub(members[next_var - 1].end_position().row);
                if gap >= 2 {
                    break;
                }
            }
            next_var += 2;
        }
        next_var
    }

    fn member_default_align_targets(
        &self,
        members: &[Node],
        colon_targets: &[Option<usize>],
    ) -> Vec<Option<usize>> {
        let mut targets = vec![None; members.len()];
        if !self.align_member_colons {
            return targets;
        }
        let indent_width = self.level * self.indent_unit.len();
        let mut idx = 0;
        while idx < members.len() {
            if !self.is_mergeable_default_pair(members, idx) {
                idx += 1;
                continue;
            }
            let run_end = self.same_line_default_pair_run_end(members, idx);
            let pair_count = (run_end - idx) / 2;
            if pair_count >= 2 {
                let width = (0..pair_count)
                    .map(|k| {
                        let var_idx = idx + k * 2;
                        self.member_var_line_width_to_semicolon(
                            members[var_idx],
                            colon_targets[var_idx],
                        )
                    })
                    .max()
                    .unwrap_or(0);
                let col = indent_width + width + DEFAULT_GAP.len();
                for k in 0..pair_count {
                    targets[idx + k * 2] = Some(col);
                }
            }
            idx = run_end;
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
                // Leave comments queued so the brace's flush handles them; emitting
                // here would inline a `//` comment that then swallows the block brace.
                "comment" => continue,
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
            // Defer to the brace's flush, else a `//` comment here swallows the brace.
            if child.kind() == "comment" {
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
            self.emit_block_open(*o);
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
            if member.kind() == "comment" {
                self.flush_comments_before(member.end_byte());
                continue;
            }
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
        self.flush_before_close(close.copied());
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
            self.emit_block_open(*o);
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
        let default_targets = self.member_default_align_targets(&members, &colon_targets);
        let open_row = node.start_position().row;
        let mut prev_end_row: Option<usize> = None;
        let mut prev_was_comment = false;
        let mut prev_member: Option<Node> = None;

        let mut idx = 0;
        while idx < members.len() {
            let member = members[idx];
            let child_row = member.start_position().row;
            let source_gap = match prev_end_row {
                Some(prev) => child_row.saturating_sub(prev),
                None => child_row.saturating_sub(open_row),
            };
            let is_callable = matches!(member.kind(), "func_decl" | "event_decl");
            let both_bodiless = is_bodiless_callable(member)
                && prev_member.map(is_bodiless_callable).unwrap_or(false);
            let want_blank = source_gap >= 2
                || (is_callable && prev_end_row.is_some() && !prev_was_comment && !both_bodiless);
            prev_was_comment = member.kind() == "comment";

            if member.kind() == "comment" {
                let trailing = self.is_trailing_comment(prev_member, member);
                if want_blank && !trailing {
                    self.nl();
                }
                self.flush_comments_before(member.end_byte());
                prev_end_row = Some(member.end_position().row);
                prev_member = Some(member);
                idx += 1;
                continue;
            }
            if want_blank {
                self.nl();
            }

            if member.kind() == "member_var_decl"
                && !self.renders_verbatim(member)
                && idx + 1 < members.len()
                && members[idx + 1].kind() == "member_default_val"
                && self.default_on_same_line(member, members[idx + 1])
            {
                self.format_member_var_decl(
                    member,
                    colon_targets[idx],
                    Some((members[idx + 1], default_targets[idx])),
                );
                prev_end_row = Some(members[idx + 1].end_position().row);
                prev_member = Some(member);
                idx += 2;
            } else {
                self.format_class_member(member, colon_targets[idx]);
                prev_end_row = Some(member.end_position().row);
                prev_member = Some(member);
                idx += 1;
            }
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
        self.nl();
    }

    // Reformatting a member with an inner comment would relocate or drop it; emit verbatim.
    fn renders_verbatim(&self, node: Node) -> bool {
        if node.is_error() {
            return true;
        }
        let mut c = node.walk();
        let has_comment_child = node.children(&mut c).any(|n| n.kind() == "comment");
        has_comment_child
    }

    fn format_class_member(&mut self, node: Node, colon_align_col: Option<usize>) {
        self.flush_comments_before(node.start_byte());
        if self.renders_verbatim(node) {
            let t = self.text(node).trim().to_string();
            self.emit_indent();
            self.emit(&t);
            self.consume_comments_before(node.end_byte());
            self.nl();
            return;
        }
        match node.kind() {
            "func_decl" | "event_decl" => self.format_func_decl(node),
            "member_default_val_block" => self.format_defaults_block(node),
            "member_default_val" => {
                self.emit_indent();
                self.format_children(node);
                self.nl();
            }
            "member_var_decl" => self.format_member_var_decl(node, colon_align_col, None),
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
            if member.kind() == "comment" {
                self.flush_comments_before(member.end_byte());
                continue;
            }
            self.emit_indent();
            if member.kind() == "member_default_val_block_assign" {
                self.format_children(*member);
            } else {
                self.emit_verbatim(*member);
            }
            self.nl();
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
        self.nl();
    }
}
