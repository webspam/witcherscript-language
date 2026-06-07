use tree_sitter::Node;

use super::super::{child_nodes, Formatter, LayoutDirective, SwitchToggle};

// Spaces between aligned switch-arm columns (label -> statement -> break).
const SWITCH_CELL_GAP: usize = 2;

// One `case`/`default` group: the labels (>1 only for stacked fall-through) and the
// statements bound to the last label.
struct SwitchArm<'t> {
    labels: Vec<Node<'t>>,
    stmts: Vec<Node<'t>>,
}

// Per-arm layout decision. `cols[i]` is the target column for statement `i` when inline.
struct ArmLayout {
    inline: bool,
    cols: Vec<usize>,
}

// A label after statements closes the current arm; runs of bare labels (fall-through)
// accumulate, with statements binding to the last label.
fn collect_switch_arms<'t>(children: &[Node<'t>]) -> Vec<SwitchArm<'t>> {
    let mut arms: Vec<SwitchArm<'t>> = Vec::new();
    let mut current: Option<SwitchArm<'t>> = None;
    for child in children {
        match child.kind() {
            "switch_case_label" | "switch_default_label" => {
                if current.as_ref().is_some_and(|a| !a.stmts.is_empty()) {
                    // `is_some_and` above guarantees `current` is `Some` here.
                    arms.push(current.take().unwrap());
                }
                current
                    .get_or_insert_with(|| SwitchArm {
                        labels: Vec::new(),
                        stmts: Vec::new(),
                    })
                    .labels
                    .push(*child);
            }
            "comment" => {}
            _ if child.is_named() => {
                current
                    .get_or_insert_with(|| SwitchArm {
                        labels: Vec::new(),
                        stmts: Vec::new(),
                    })
                    .stmts
                    .push(*child);
            }
            _ => {}
        }
    }
    if let Some(arm) = current {
        arms.push(arm);
    }
    arms
}

fn arm_start_row(arm: &SwitchArm) -> Option<usize> {
    arm.labels
        .first()
        .or_else(|| arm.stmts.first())
        .map(|n| n.start_position().row)
}

fn arm_end_row(arm: &SwitchArm) -> Option<usize> {
    arm.stmts
        .last()
        .or_else(|| arm.labels.last())
        .map(|n| n.end_position().row)
}

fn blank_line_between_arms(a: &SwitchArm, b: &SwitchArm) -> bool {
    match (arm_end_row(a), arm_start_row(b)) {
        (Some(end), Some(start)) => start.saturating_sub(end) >= 2,
        _ => false,
    }
}

pub(super) fn format_switch_stmt(f: &mut Formatter<'_>, node: Node) {
    f.format_switch_stmt_impl(node);
}

impl<'a> Formatter<'a> {
    pub(in crate::formatter) fn format_switch_stmt_impl(&mut self, node: Node) {
        let cond = node.child_by_field_name("cond");
        if self.emit_split_keyword_cond("switch (", cond) {
            self.emit(" {\n");
        } else {
            self.emit_indent();
            self.emit("switch (");
            if let Some(c) = cond {
                self.format_node(c);
            }
            self.emit(") {\n");
        }
        self.level += 1;
        let mut close: Option<Node> = None;
        if let Some(block) = self.child_of_kind(node, "switch_block") {
            let children = child_nodes(block);
            close = children.iter().rfind(|n| n.kind() == "}").copied();
            let arms = collect_switch_arms(&children);
            let layouts = self.switch_arm_layouts(&arms);
            let mut prev: Option<&SwitchArm> = None;
            for (arm, layout) in arms.iter().zip(layouts.iter()) {
                if let Some(p) = prev {
                    // A comment between arms owns its own row, so it alone is not a blank line.
                    if blank_line_between_arms(p, arm) && !self.comment_between_arms(p, arm) {
                        self.nl();
                    }
                }
                if layout.inline {
                    self.emit_switch_arm_inline(arm, &layout.cols);
                } else {
                    self.emit_switch_arm_block(arm);
                }
                prev = Some(arm);
            }
        }
        self.flush_before_close(close);
        self.level -= 1;
        self.emit_indent();
        self.emit("}\n");
    }

    fn emit_switch_arm_inline(&mut self, arm: &SwitchArm, cols: &[usize]) {
        let split = arm.labels.len() - 1;
        for label in &arm.labels[..split] {
            self.flush_comments_before(label.start_byte());
            self.emit_indent();
            self.format_children(*label);
            self.nl();
        }
        let last_label = arm.labels[split];
        self.flush_comments_before(last_label.start_byte());
        self.emit_indent();
        self.format_children(last_label);
        for (i, stmt) in arm.stmts.iter().enumerate() {
            while self.current_line_len() < cols[i] {
                self.emit(" ");
            }
            let rendered = self.render_node(*stmt);
            self.emit(&rendered);
        }
        if let Some(last_stmt) = arm.stmts.last() {
            self.consume_comments_before(last_stmt.end_byte());
        }
        self.nl();
    }

    fn emit_switch_arm_block(&mut self, arm: &SwitchArm) {
        for label in &arm.labels {
            self.flush_comments_before(label.start_byte());
            self.emit_indent();
            self.format_children(*label);
            self.nl();
        }
        self.level += 1;
        for stmt in &arm.stmts {
            self.format_stmt(*stmt);
        }
        self.level -= 1;
    }

    fn switch_arm_layouts(&self, arms: &[SwitchArm]) -> Vec<ArmLayout> {
        match self.layout_directive {
            Some(LayoutDirective::SwitchExpand) => return self.expanded_arm_layouts(arms),
            Some(LayoutDirective::SwitchCollapse) => return self.collapsed_arm_layouts(arms),
            None => {}
        }
        let mut layouts: Vec<ArmLayout> = arms
            .iter()
            .map(|_| ArmLayout {
                inline: false,
                cols: Vec::new(),
            })
            .collect();
        let indent_width = self.level * self.indent_unit.len();
        let mut i = 0;
        while i < arms.len() {
            if !self.switch_arm_structurally_inline(&arms[i]) {
                i += 1;
                continue;
            }
            let mut j = i;
            while j + 1 < arms.len() && self.switch_arm_structurally_inline(&arms[j + 1]) {
                j += 1;
            }
            self.assign_run_layout(&arms[i..=j], &mut layouts[i..=j], indent_width);
            i = j + 1;
        }
        layouts
    }

    fn assign_run_layout(&self, run: &[SwitchArm], layouts: &mut [ArmLayout], indent_width: usize) {
        // A run holds only structurally-inline arms, which always have a last label.
        let label_w = run
            .iter()
            .map(|a| self.render_node(*a.labels.last().unwrap()).len())
            .max()
            .unwrap_or(0);
        let max_cells = run.iter().map(|a| a.stmts.len()).max().unwrap_or(0);
        let mut cell_w = vec![0usize; max_cells];
        for arm in run {
            for (k, stmt) in arm.stmts.iter().enumerate() {
                cell_w[k] = cell_w[k].max(self.render_node(*stmt).len());
            }
        }
        // A lone arm has nothing to align with, so it uses a plain single-space gap.
        let gap = if run.len() == 1 { 1 } else { SWITCH_CELL_GAP };
        let mut cols = vec![0usize; max_cells];
        let mut col = indent_width + label_w + gap;
        for (k, target) in cols.iter_mut().enumerate() {
            *target = col;
            col += cell_w[k] + gap;
        }
        // Padding can push a line past the limit, so demote on the ALIGNED width.
        let widest_end = run
            .iter()
            .flat_map(|arm| {
                arm.stmts
                    .iter()
                    .enumerate()
                    .map(|(k, stmt)| cols[k] + self.render_node(*stmt).len())
            })
            .max()
            .unwrap_or(0);
        let inline = widest_end <= self.line_limit;
        for layout in layouts.iter_mut() {
            layout.inline = inline;
            layout.cols = cols.clone();
        }
    }

    fn switch_arm_structurally_inline(&self, arm: &SwitchArm) -> bool {
        let Some(last_label) = arm.labels.last() else {
            return false;
        };
        if arm.stmts.is_empty() {
            return false;
        }
        let row = last_label.start_position().row;
        if last_label.end_position().row != row {
            return false;
        }
        let same_row = arm
            .stmts
            .iter()
            .all(|s| s.start_position().row == row && s.end_position().row == row);
        if !same_row {
            return false;
        }
        let non_break = arm
            .stmts
            .iter()
            .filter(|s| s.kind() != "break_stmt")
            .count();
        if non_break > 1 {
            return false;
        }
        !self.arm_has_interior_comment(arm)
    }

    fn arm_has_interior_comment(&self, arm: &SwitchArm) -> bool {
        let (Some(start_row), Some(end_row)) = (arm_start_row(arm), arm_end_row(arm)) else {
            return false;
        };
        self.comments
            .iter()
            .any(|c| (start_row..=end_row).contains(&c.start_position().row))
    }

    fn comment_between_arms(&self, a: &SwitchArm, b: &SwitchArm) -> bool {
        let a_end = a
            .stmts
            .last()
            .or_else(|| a.labels.last())
            .map(|n| n.end_byte());
        let b_start = b
            .labels
            .first()
            .or_else(|| b.stmts.first())
            .map(|n| n.start_byte());
        match (a_end, b_start) {
            (Some(e), Some(s)) => self
                .comments
                .iter()
                .any(|c| c.start_byte() >= e && c.start_byte() < s),
            _ => false,
        }
    }

    fn expanded_arm_layouts(&self, arms: &[SwitchArm]) -> Vec<ArmLayout> {
        arms.iter()
            .map(|_| ArmLayout {
                inline: false,
                cols: Vec::new(),
            })
            .collect()
    }

    // Every arm forms one aligned run; assign_run_layout still demotes to block past the line limit.
    fn collapsed_arm_layouts(&self, arms: &[SwitchArm]) -> Vec<ArmLayout> {
        let mut layouts = self.expanded_arm_layouts(arms);
        if !arms.is_empty() {
            let indent_width = self.level * self.indent_unit.len();
            self.assign_run_layout(arms, &mut layouts, indent_width);
        }
        layouts
    }

    // Like switch_arm_structurally_inline but without requiring the statements to already share
    // the label's row: the test for whether the arm *can* be joined onto one line.
    fn collapsible_arm(&self, arm: &SwitchArm) -> bool {
        let Some(last_label) = arm.labels.last() else {
            return false;
        };
        if last_label.start_position().row != last_label.end_position().row {
            return false;
        }
        let each_single_line = arm
            .stmts
            .iter()
            .all(|s| s.start_position().row == s.end_position().row);
        if !each_single_line {
            return false;
        }
        let non_break = arm
            .stmts
            .iter()
            .filter(|s| s.kind() != "break_stmt")
            .count();
        if non_break > 1 {
            return false;
        }
        !self.arm_has_interior_comment(arm)
    }

    pub(in crate::formatter) fn switch_toggle(&self, switch_node: Node) -> SwitchToggle {
        let Some(block) = self.child_of_kind(switch_node, "switch_block") else {
            return SwitchToggle {
                can_collapse: false,
                can_expand: false,
            };
        };
        let children = child_nodes(block);
        let arms = collect_switch_arms(&children);
        let stmt_arms: Vec<&SwitchArm> = arms.iter().filter(|a| !a.stmts.is_empty()).collect();
        let any_inline = stmt_arms
            .iter()
            .any(|a| self.switch_arm_structurally_inline(a));
        let any_block = stmt_arms
            .iter()
            .any(|a| !self.switch_arm_structurally_inline(a));
        let all_collapsible = stmt_arms.iter().all(|a| self.collapsible_arm(a));
        // Collapse only when the aligned single-line result stays inline; otherwise the formatter
        // would re-expand it, so offering collapse would produce output `just fmt` undoes.
        let width_ok = all_collapsible && {
            let layouts = self.collapsed_arm_layouts(&arms);
            arms.iter()
                .zip(&layouts)
                .all(|(a, l)| a.stmts.is_empty() || l.inline)
        };
        SwitchToggle {
            can_collapse: all_collapsible && width_ok && any_block,
            can_expand: any_inline,
        }
    }
}
