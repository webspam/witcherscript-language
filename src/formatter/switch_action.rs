use std::fmt::Write;

use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::kinds;
use crate::cst::offsets::nodes_at_offset;

use super::action::{Substitution, indent_unit_for, layout_ctx, line_indent, splice_subs};
use super::statements::{SwitchArm, collect_switch_arms};
use super::{FormatOptions, child_nodes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchLayout {
    Collapse,
    Expand,
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
        .find_map(|n| find_ancestor_of_kind(n, &[kinds::SWITCH_STMT]))
}

pub fn analyze_switch(switch_node: Node, options: FormatOptions) -> SwitchToggle {
    layout_ctx(switch_node, &options).switch_toggle(switch_node)
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
    let base = line_indent(source, switch_node.start_byte());
    let subs = arm_substitutions(switch_node, source, layout, base, &unit);
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
    base: &str,
    unit: &str,
) -> Vec<Substitution> {
    let Some(block) = child_nodes(switch_node)
        .into_iter()
        .find(|n| n.kind() == kinds::SWITCH_BLOCK)
    else {
        return Vec::new();
    };
    let children = child_nodes(block);
    collect_switch_arms(&children)
        .iter()
        .filter_map(|arm| arm_substitution(arm, source, layout, base, unit))
        .collect()
}

fn arm_substitution(
    arm: &SwitchArm,
    source: &str,
    layout: SwitchLayout,
    base: &str,
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
            arm.stmts.iter().fold(String::new(), |mut acc, s| {
                let _ = write!(acc, " {}", &source[s.start_byte()..s.end_byte()]);
                acc
            })
        }
        SwitchLayout::Expand => {
            if !arm_is_inline(arm) {
                return None;
            }
            let indent = format!("{base}{unit}{unit}");
            arm.stmts.iter().fold(String::new(), |mut acc, s| {
                let _ = write!(acc, "\n{indent}{}", &source[s.start_byte()..s.end_byte()]);
                acc
            })
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
