use lsp_types::{CodeActionKind, CodeActionOrCommand, Diagnostic, NumberOrString, Range, Url};
use rstest::rstest;
use serde_json::json;
use witcherscript_language::document::parse_document;
use witcherscript_language::formatter::{switch_stmt_on_keyword, FormatOptions};

use crate::convert::{base_script_conflict_code_actions, infer_indent, switch_layout_code_actions};

fn diag(code: Option<&str>, data: Option<serde_json::Value>) -> Diagnostic {
    Diagnostic {
        range: Range::default(),
        code: code.map(|c| NumberOrString::String(c.to_string())),
        data,
        ..Diagnostic::default()
    }
}

#[test]
fn emits_quickfix_for_base_script_conflict() {
    let actions = base_script_conflict_code_actions(
        &[diag(
            Some("base_script_conflict"),
            Some(json!({ "directory": "D:\\MyMod\\scripts" })),
        )],
        &[],
    );
    assert_eq!(actions.len(), 1);
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));
    assert_eq!(action.diagnostics.as_ref().map(|d| d.len()), Some(1));
    assert!(
        action.title.contains("D:\\MyMod\\scripts")
            && action.title.contains("legacyScriptDirectories"),
        "unexpected title: {}",
        action.title,
    );
    let command = action
        .command
        .as_ref()
        .expect("quickfix must carry a command");
    assert_eq!(command.command, "witcherscript.addLegacyScriptDirectory");
    assert_eq!(
        command.arguments.as_ref().unwrap(),
        &vec![json!("D:\\MyMod\\scripts")],
    );
}

#[test]
fn many_conflicts_in_one_directory_yield_a_single_quickfix() {
    let actions = base_script_conflict_code_actions(
        &[
            diag(
                Some("base_script_conflict"),
                Some(json!({ "directory": "D:\\MyMod\\scripts" })),
            ),
            diag(
                Some("base_script_conflict"),
                Some(json!({ "directory": "D:\\MyMod\\scripts" })),
            ),
        ],
        &[],
    );
    assert_eq!(
        actions.len(),
        1,
        "one action per directory, not per declaration",
    );
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    assert_eq!(
        action.diagnostics.as_ref().map(|d| d.len()),
        Some(2),
        "the action should claim both conflict diagnostics",
    );
}

#[rstest]
#[case::unrelated_diagnostic_code(
    Some("duplicate_symbol"),
    Some(json!({ "directory": "D:\\x" })),
)]
#[case::base_script_conflict_without_data(Some("base_script_conflict"), None)]
#[case::data_missing_directory_key(
    Some("base_script_conflict"),
    Some(json!({ "other": "x" })),
)]
#[case::no_diagnostic_code(None, Some(json!({ "directory": "D:\\x" })))]
fn no_quickfix_when_not_applicable(
    #[case] code: Option<&str>,
    #[case] data: Option<serde_json::Value>,
) {
    let actions = base_script_conflict_code_actions(&[diag(code, data)], &[]);
    assert!(
        actions.is_empty(),
        "expected no code actions, got {actions:?}"
    );
}

const BLOCK_SWITCH: &str = "function F() {\n    switch (x) {\n        case 0:\n            Foo();\n            break;\n        case 1:\n            Bar();\n            break;\n    }\n}\n";

const INLINE_SWITCH: &str =
    "function F() {\n    switch (x) {\n        case 0:  Foo();  break;\n        case 1:  Bar();  break;\n    }\n}\n";

const MIXED_SWITCH: &str = "function F() {\n    switch (x) {\n        case 0:  Foo();  break;\n        case 1:\n            Bar();\n            break;\n    }\n}\n";

fn switch_actions(src: &str, needle: &str) -> Vec<CodeActionOrCommand> {
    let doc = parse_document(src).expect("should parse");
    let byte = src.find(needle).expect("needle present") + 1;
    let Some(switch_node) = switch_stmt_on_keyword(doc.tree.root_node(), byte) else {
        return Vec::new();
    };
    let (use_tabs, tab_size) = infer_indent(&doc.source, switch_node);
    let options = FormatOptions {
        tab_size,
        use_tabs,
        ..FormatOptions::default()
    };
    let uri = Url::parse("file:///main.ws").unwrap();
    switch_layout_code_actions(&uri, &doc, switch_node, options)
}

fn titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
    actions
        .iter()
        .map(|a| match a {
            CodeActionOrCommand::CodeAction(action) => action.title.clone(),
            CodeActionOrCommand::Command(cmd) => cmd.title.clone(),
        })
        .collect()
}

fn new_text(action: &CodeActionOrCommand) -> String {
    let CodeActionOrCommand::CodeAction(action) = action else {
        panic!("expected a CodeAction");
    };
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_REWRITE));
    let edits = action
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.values().next())
        .expect("rewrite carries a WorkspaceEdit");
    edits[0].new_text.clone()
}

#[rstest]
#[case::on_switch_keyword("switch")]
#[case::on_case_keyword("case")]
fn block_switch_offers_only_collapse(#[case] needle: &str) {
    let actions = switch_actions(BLOCK_SWITCH, needle);
    assert_eq!(
        titles(&actions),
        vec!["Collapse switch cases to a single line"],
        "block switch should offer collapse only",
    );
    assert!(new_text(&actions[0]).contains("case 0:  Foo();  break;"));
}

#[test]
fn inline_switch_offers_only_expand() {
    let actions = switch_actions(INLINE_SWITCH, "switch");
    assert_eq!(
        titles(&actions),
        vec!["Expand switch cases onto multiple lines"],
        "inline switch should offer expand only",
    );
}

#[test]
fn mixed_switch_offers_collapse_first_and_preferred() {
    let actions = switch_actions(MIXED_SWITCH, "switch");
    assert_eq!(
        titles(&actions),
        vec![
            "Collapse switch cases to a single line",
            "Expand switch cases onto multiple lines",
        ],
        "mix should offer collapse first, then expand",
    );
    let CodeActionOrCommand::CodeAction(collapse) = &actions[0] else {
        panic!("expected a CodeAction");
    };
    assert_eq!(
        collapse.is_preferred,
        Some(true),
        "collapse is the default in a mix",
    );
}

#[test]
fn no_switch_actions_off_a_keyword() {
    let actions = switch_actions(BLOCK_SWITCH, "Foo");
    assert!(actions.is_empty(), "cursor off a keyword offers nothing");
}
