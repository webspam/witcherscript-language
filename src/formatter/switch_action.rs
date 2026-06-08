use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::offsets::nodes_at_offset;

use super::action::{formatter_for, indent_unit_for, node_indent_level, splice_subs, Substitution};
use super::statements::{collect_switch_arms, SwitchArm};
use super::{child_nodes, collect_comments, FormatOptions, LayoutDirective};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchLayout {
    Collapse,
    Expand,
}

impl From<SwitchLayout> for LayoutDirective {
    fn from(layout: SwitchLayout) -> Self {
        match layout {
            SwitchLayout::Collapse => LayoutDirective::Collapse,
            SwitchLayout::Expand => LayoutDirective::Expand,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchToggle {
    pub can_collapse: bool,
    pub can_expand: bool,
}

/// The enclosing `switch_stmt` when `byte` sits anywhere inside one, else `None`.
pub fn switch_stmt_at(root: Node, byte: usize) -> Option<Node> {
    nodes_at_offset(root, byte)
        .into_iter()
        .find_map(|n| find_ancestor_of_kind(n, &["switch_stmt"]))
}

pub fn analyze_switch(switch_node: Node, source: &str, options: FormatOptions) -> SwitchToggle {
    let comments = collect_comments(switch_node);
    let level = node_indent_level(switch_node, &options);
    let f = formatter_for(source, options, comments, level, None);
    f.switch_toggle(switch_node)
}

/// Verbatim structural rewrite of `switch_node` to `layout`: each arm's statements are joined onto
/// the label line (collapse) or split onto their own indented lines (expand). Statement text is
/// copied byte-for-byte and column alignment is left to the formatter, run separately.
pub fn rewrite_switch_layout(
    switch_node: Node,
    source: &str,
    options: FormatOptions,
    layout: SwitchLayout,
) -> String {
    let unit = indent_unit_for(&options);
    let level = node_indent_level(switch_node, &options);
    let subs = arm_substitutions(switch_node, source, layout, level, &unit);
    splice_subs(
        source,
        switch_node.start_byte(),
        switch_node.end_byte(),
        subs,
    )
}

fn arm_substitutions(
    switch_node: Node,
    source: &str,
    layout: SwitchLayout,
    level: usize,
    unit: &str,
) -> Vec<Substitution> {
    let Some(block) = child_nodes(switch_node)
        .into_iter()
        .find(|n| n.kind() == "switch_block")
    else {
        return Vec::new();
    };
    let children = child_nodes(block);
    collect_switch_arms(&children)
        .iter()
        .filter_map(|arm| arm_substitution(arm, source, layout, level, unit))
        .collect()
}

fn arm_substitution(
    arm: &SwitchArm,
    source: &str,
    layout: SwitchLayout,
    level: usize,
    unit: &str,
) -> Option<Substitution> {
    let last_label = arm.labels.last()?;
    let last_stmt = arm.stmts.last()?;
    let (start, end) = (last_label.end_byte(), last_stmt.end_byte());
    let text = match layout {
        // An already-inline arm has nothing to join; leave it verbatim.
        SwitchLayout::Collapse => {
            if arm_is_inline(arm) {
                return None;
            }
            arm.stmts
                .iter()
                .map(|s| format!(" {}", &source[s.start_byte()..s.end_byte()]))
                .collect()
        }
        SwitchLayout::Expand => {
            if !arm_is_inline(arm) {
                return None;
            }
            let indent = unit.repeat(level + 2);
            arm.stmts
                .iter()
                .map(|s| format!("\n{indent}{}", &source[s.start_byte()..s.end_byte()]))
                .collect()
        }
    };
    Some(Substitution { start, end, text })
}

// True when the last label and all its statements already sit on one source row.
fn arm_is_inline(arm: &SwitchArm) -> bool {
    let Some(last_label) = arm.labels.last() else {
        return false;
    };
    let row = last_label.start_position().row;
    last_label.end_position().row == row
        && arm
            .stmts
            .iter()
            .all(|s| s.start_position().row == row && s.end_position().row == row)
}
