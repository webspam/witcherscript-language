use tree_sitter::Node;

use crate::cst::kinds;

use super::{Formatter, child_nodes, is_alignable_field, is_bodiless_callable};

const DEFAULT_GAP: &str = "  ";

impl Formatter<'_> {
    // ---- Top level ----

    pub(super) fn format_script(&mut self, node: Node) {
        let children: Vec<Node> = child_nodes(node)
            .into_iter()
            .filter(|n| n.is_named() && n.kind() != kinds::NOP)
            .collect();
        let mut prev_end_row: Option<usize> = None;
        let mut prev_was_comment = false;
        let mut prev_node: Option<Node> = None;
        for child in &children {
            let child_row = child.start_position().row;
            let child_is_comment = child.kind() == kinds::COMMENT;
            let trailing = child_is_comment && self.is_trailing_comment(prev_node, *child);
            if let Some(prev) = prev_end_row {
                let source_gap = child_row.saturating_sub(prev);
                // Consecutive single-line @addField members may sit gaplessly; do not force a blank.
                let both_add_field = prev_node.is_some_and(|p| self.is_add_field_decl(p))
                    && self.is_add_field_decl(*child);
                if !trailing
                    && (source_gap >= 2
                        || (!both_add_field && !prev_was_comment && !child_is_comment))
                {
                    self.nl();
                }
            }
            prev_was_comment = child.kind() == kinds::COMMENT;
            if child.kind() == kinds::COMMENT {
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
            if child.is_missing() || child.kind() == kinds::ANNOTATION {
                continue;
            }
            return child.start_position().row == ann_row;
        }
        false
    }

    fn is_add_field_annotation(&self, ann: Node) -> bool {
        self.child_of_kind(ann, kinds::ANNOTATION_IDENT)
            .is_some_and(|ident| self.text(ident).trim_start_matches('@') == "addField")
    }

    fn is_add_field_decl(&self, node: Node) -> bool {
        node.kind() == kinds::MEMBER_VAR_DECL
            && self
                .child_of_kind(node, kinds::ANNOTATION)
                .is_some_and(|ann| self.is_add_field_annotation(ann))
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
        self.child_of_kind(node, kinds::IDENT).map(|n| self.text(n))
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
        let same_line = if let Some(ann) = self.child_of_kind(node, kinds::ANNOTATION) {
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
            if child.is_missing() || child.kind() == kinds::ANNOTATION {
                continue;
            }
            if let Some(p) = prev
                && self.gap_between(p, *child, parent_kind)
            {
                width += 1;
            }
            width += self.render_node(*child).len();
            prev = Some(*child);
        }
        width
    }

    fn member_var_line_width_to_semicolon(
        &self,
        node: Node,
        colon_align_col: Option<usize>,
    ) -> usize {
        let unaligned_width = self.member_var_decl_width(node);
        let Some(col) = colon_align_col else {
            return unaligned_width;
        };
        let indent_width = self.level * self.indent_unit.len();
        let alignment_pad =
            col.saturating_sub(indent_width + self.member_var_pre_colon_width(node));
        unaligned_width + alignment_pad
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
                let run = self.same_line_default_pair_run(members, idx);
                if run.len() >= 2 {
                    let width = run
                        .iter()
                        .map(|&v| self.member_var_pre_colon_width(members[v]))
                        .max()
                        .unwrap_or(0);
                    let col = indent_width + width;
                    for &v in &run {
                        targets[v] = Some(col);
                    }
                }
                idx = run[run.len() - 1] + 2;
            } else if is_alignable_field(members[idx]) {
                let run = self.colon_align_run(members, idx);
                if run.len() >= 2 {
                    let width = run
                        .iter()
                        .map(|&k| self.member_var_pre_colon_width(members[k]))
                        .max()
                        .unwrap_or(0);
                    for &k in &run {
                        targets[k] = Some(indent_width + width);
                    }
                }
                idx = run[run.len() - 1] + 1;
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
        members[var_idx].kind() == kinds::MEMBER_VAR_DECL
            && default.kind() == kinds::MEMBER_DEFAULT_VAL
            && is_alignable_field(members[var_idx])
            && self.default_on_same_line(members[var_idx], *default)
    }

    // A comment between members doesn't break the run; a blank line does.
    fn alignment_run(
        &self,
        members: &[Node],
        run_start: usize,
        stride: usize,
        is_run_member: impl Fn(&[Node], usize) -> bool,
    ) -> Vec<usize> {
        let mut run = vec![run_start];
        let mut prev = run_start + stride - 1;
        let mut scan = run_start + stride;
        while scan < members.len() {
            let gap = members[scan]
                .start_position()
                .row
                .saturating_sub(members[prev].end_position().row);
            if gap >= 2 {
                break;
            }
            if members[scan].kind() == kinds::COMMENT {
                prev = scan;
                scan += 1;
                continue;
            }
            if !is_run_member(members, scan) {
                break;
            }
            run.push(scan);
            prev = scan + stride - 1;
            scan += stride;
        }
        run
    }

    fn colon_align_run(&self, members: &[Node], run_start: usize) -> Vec<usize> {
        self.alignment_run(members, run_start, 1, |m, i| {
            is_alignable_field(m[i]) && !self.is_mergeable_default_pair(m, i)
        })
    }

    fn same_line_default_pair_run(&self, members: &[Node], run_start: usize) -> Vec<usize> {
        self.alignment_run(members, run_start, 2, |m, i| {
            self.is_mergeable_default_pair(m, i)
        })
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
            let run = self.same_line_default_pair_run(members, idx);
            if run.len() >= 2 {
                let width = run
                    .iter()
                    .map(|&v| self.member_var_line_width_to_semicolon(members[v], colon_targets[v]))
                    .max()
                    .unwrap_or(0);
                let col = indent_width + width + DEFAULT_GAP.len();
                for &v in &run {
                    targets[v] = Some(col);
                }
            }
            idx = run[run.len() - 1] + 2;
        }
        targets
    }

    pub(super) fn format_func_decl(&mut self, node: Node) {
        if let Some(ann) = self.child_of_kind(node, kinds::ANNOTATION) {
            self.emit_annotation(ann);
        }
        self.emit_indent();

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
            match child.kind() {
                kinds::FUNC_PARAMS => {
                    self.format_func_params(node);
                    prev = Some(*child);
                    continue;
                }
                kinds::FUNC_BLOCK => {
                    self.format_func_block(*child);
                    return;
                }
                kinds::NOP => {
                    let t = self.text(*child).to_string();
                    self.emit(&t);
                    self.nl();
                    return;
                }
                _ => {}
            }
            if let Some(p) = prev
                && !self.out.ends_with('\n')
                && self.gap_between(p, *child, node.kind())
            {
                self.emit(" ");
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
            if child.kind() == kinds::COMMENT {
                continue;
            }
            if child.kind() == kinds::ENUM_DEF {
                self.emit(" ");
                self.format_enum_def(*child);
                return;
            }
            if let Some(p) = prev
                && self.gap_between(p, *child, node.kind())
            {
                self.emit(" ");
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
        // Exhaustive: all named children - enum_decl_variant AND comment extras.
        // Anonymous tokens ({, ,, }) are excluded by is_named() and handled directly.
        let members: Vec<Node> = children.iter().filter(|n| n.is_named()).copied().collect();
        let open = children.iter().find(|n| n.kind() == "{");
        let close = children.iter().rfind(|n| n.kind() == "}");

        if let Some(o) = open {
            self.emit_block_open(*o);
        }
        if members.is_empty() {
            if let Some(cl) = close
                && !cl.is_missing()
            {
                self.emit_verbatim(*cl);
            }
            self.nl();
            return;
        }
        self.nl();
        self.level += 1;
        let member_count = members
            .iter()
            .filter(|n| n.kind() == kinds::ENUM_DECL_VARIANT)
            .count();
        let mut emitted_members = 0;
        for member in &members {
            if member.kind() == kinds::COMMENT {
                self.flush_comments_before(member.end_byte());
                continue;
            }
            self.emit_indent();
            if member.kind() == kinds::ENUM_DECL_VARIANT {
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
        if let Some(cl) = close
            && !cl.is_missing()
        {
            self.emit_verbatim(*cl);
        }
        self.nl();
    }

    pub(super) fn format_class_def(&mut self, node: Node) {
        let children = child_nodes(node);
        let members: Vec<Node> = children
            .iter()
            .filter(|n| n.is_named() && n.kind() != kinds::NOP)
            .copied()
            .collect();
        let open = children.iter().find(|n| n.kind() == "{");
        let close = children.iter().rfind(|n| n.kind() == "}");

        if let Some(o) = open {
            self.emit_block_open(*o);
        }
        if members.is_empty() {
            if let Some(cl) = close
                && !cl.is_missing()
            {
                let t = self.text(*cl).to_string();
                self.emit(&t);
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
            let is_callable = matches!(member.kind(), kinds::FUNC_DECL | kinds::EVENT_DECL);
            let both_bodiless =
                is_bodiless_callable(member) && prev_member.is_some_and(is_bodiless_callable);
            let want_blank = source_gap >= 2
                || (is_callable && prev_end_row.is_some() && !prev_was_comment && !both_bodiless);
            prev_was_comment = member.kind() == kinds::COMMENT;

            if member.kind() == kinds::COMMENT {
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

            if member.kind() == kinds::MEMBER_VAR_DECL
                && !self.renders_verbatim(member)
                && idx + 1 < members.len()
                && members[idx + 1].kind() == kinds::MEMBER_DEFAULT_VAL
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
        if let Some(cl) = close
            && !cl.is_missing()
        {
            let t = self.text(*cl).to_string();
            self.emit(&t);
        }
        self.nl();
    }

    // Reformatting a member with an inner comment would relocate or drop it; emit verbatim.
    fn renders_verbatim(&self, node: Node) -> bool {
        if node.is_error() {
            return true;
        }
        let mut c = node.walk();

        node.children(&mut c).any(|n| n.kind() == kinds::COMMENT)
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
            kinds::FUNC_DECL | kinds::EVENT_DECL => self.format_func_decl(node),
            kinds::MEMBER_DEFAULT_VAL_BLOCK => self.format_defaults_block(node),
            kinds::MEMBER_VAR_DECL => self.format_member_var_decl(node, colon_align_col, None),
            _ => {
                self.emit_indent();
                self.format_children(node);
                self.nl();
            }
        }
    }

    fn format_defaults_block(&mut self, node: Node) {
        let children = child_nodes(node);
        // Exhaustive: all named children - member_default_val_block_assign AND
        // comment extras. The `defaults` keyword and {/} braces are anonymous
        // tokens and are excluded by is_named(), then handled directly below.
        let members: Vec<Node> = children.iter().filter(|n| n.is_named()).copied().collect();
        let open = children.iter().find(|n| n.kind() == "{");
        let close = children.iter().rfind(|n| n.kind() == "}");

        self.emit_indent();
        if let Some(kw) = children.iter().find(|n| n.kind() == "defaults")
            && !kw.is_missing()
        {
            let t = self.text(*kw).to_string();
            self.emit(&t);
        }
        self.emit(" ");

        if let Some(o) = open
            && !o.is_missing()
        {
            let t = self.text(*o).to_string();
            self.emit(&t);
        }
        if members.is_empty() {
            if let Some(cl) = close
                && !cl.is_missing()
            {
                let t = self.text(*cl).to_string();
                self.emit(&t);
            }
            self.nl();
            return;
        }
        self.nl();
        self.level += 1;
        for member in &members {
            if member.kind() == kinds::COMMENT {
                self.flush_comments_before(member.end_byte());
                continue;
            }
            self.emit_indent();
            if member.kind() == kinds::MEMBER_DEFAULT_VAL_BLOCK_ASSIGN {
                self.format_children(*member);
            } else {
                self.emit_verbatim(*member);
            }
            self.nl();
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
        self.nl();
    }
}
