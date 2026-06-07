use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Range, TextEdit, Url, WorkspaceEdit,
};
use tree_sitter::Node;
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::formatter::{
    analyze_switch, format_switch_with_layout, FormatOptions, SwitchLayout,
};

use super::lsp_range;

const COLLAPSE_TITLE: &str = "Collapse switch cases to a single line";
const EXPAND_TITLE: &str = "Expand switch cases onto multiple lines";

/// Tab style for a rewrite, inferred from the source around `switch_node` since code-action
/// requests carry no editor formatting hint.
pub(crate) fn infer_indent(source: &str, switch_node: Node) -> (bool, u32) {
    let start = switch_node.start_byte();
    let line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let use_tabs = source[line_start..start].contains('\t');
    let tab_size = label_indent_step(switch_node).unwrap_or(4) as u32;
    (use_tabs, tab_size)
}

// Per-level step in columns: the gap between the switch and its first case/default label.
fn label_indent_step(switch_node: Node) -> Option<usize> {
    let block = switch_node.child_by_field_name("body")?;
    let mut cursor = block.walk();
    let label = block
        .children(&mut cursor)
        .find(|n| matches!(n.kind(), "switch_case_label" | "switch_default_label"))?;
    label
        .start_position()
        .column
        .checked_sub(switch_node.start_position().column)
        .filter(|step| *step > 0)
}

pub(crate) fn switch_layout_code_actions(
    uri: &Url,
    document: &ParsedDocument,
    switch_node: Node,
    options: FormatOptions,
) -> Vec<CodeActionOrCommand> {
    let toggle = analyze_switch(switch_node, &document.source, options);
    if !toggle.can_collapse && !toggle.can_expand {
        return Vec::new();
    }
    let range = lsp_range(document.line_index.byte_range_to_range(
        &document.source,
        switch_node.start_byte(),
        switch_node.end_byte(),
    ));

    let mut actions = Vec::new();
    if toggle.can_collapse {
        let text = format_switch_with_layout(
            switch_node,
            &document.source,
            options,
            SwitchLayout::Collapse,
        );
        actions.push(rewrite_action(COLLAPSE_TITLE, uri, range, text, true));
    }
    if toggle.can_expand {
        let text =
            format_switch_with_layout(switch_node, &document.source, options, SwitchLayout::Expand);
        actions.push(rewrite_action(EXPAND_TITLE, uri, range, text, false));
    }
    actions
}

fn rewrite_action(
    title: &str,
    uri: &Url,
    range: Range,
    new_text: String,
    preferred: bool,
) -> CodeActionOrCommand {
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![TextEdit { range, new_text }]);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }),
        is_preferred: preferred.then_some(true),
        ..CodeAction::default()
    })
}
