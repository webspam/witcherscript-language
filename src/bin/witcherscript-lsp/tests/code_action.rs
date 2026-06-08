use lsp_types::{CodeActionKind, CodeActionOrCommand, Diagnostic, NumberOrString, Range, Url};
use rstest::rstest;
use serde_json::json;
use witcherscript_language::document::parse_document;
use witcherscript_language::formatter::FormatOptions;

use crate::convert::{base_script_conflict_code_actions, refactor_code_actions};

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

fn refactor_actions(src: &str, needle: &str) -> Vec<CodeActionOrCommand> {
    let doc = parse_document(src).expect("should parse");
    let cursor = src.find(needle).expect("needle present") + 1;
    let options = FormatOptions::default();
    let uri = Url::parse("file:///main.ws").unwrap();
    refactor_code_actions(&uri, &doc, cursor, options)
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

const COLLAPSE: &str = "Collapse switch cases to a single line";
const EXPAND: &str = "Expand switch cases onto multiple lines";

#[rstest]
#[case::block_on_switch(
    include_str!("../../../../tests/fixtures/formatter/switch_block.ws"),
    "switch",
    &[COLLAPSE]
)]
#[case::block_on_case(
    include_str!("../../../../tests/fixtures/formatter/switch_block.ws"),
    "case",
    &[COLLAPSE]
)]
#[case::inline_only_expand(
    include_str!("../../../../tests/fixtures/formatter/switch_inline.ws"),
    "switch",
    &[EXPAND]
)]
#[case::mixed_collapse_first(
    include_str!("../../../../tests/fixtures/formatter/switch_mixed.ws"),
    "switch",
    &[COLLAPSE, EXPAND]
)]
#[case::on_condition(
    include_str!("../../../../tests/fixtures/formatter/switch_block.ws"),
    "(x)",
    &[COLLAPSE]
)]
#[case::on_statement(
    include_str!("../../../../tests/fixtures/formatter/switch_block.ws"),
    "Foo",
    &[COLLAPSE]
)]
#[case::outside_switch(
    include_str!("../../../../tests/fixtures/formatter/switch_block.ws"),
    "function",
    &[]
)]
fn offers_expected_refactor_actions(
    #[case] src: &str,
    #[case] needle: &str,
    #[case] expected: &[&str],
) {
    let actions = refactor_actions(src, needle);
    let title_list = titles(&actions);
    let got: Vec<&str> = title_list.iter().map(String::as_str).collect();
    assert_eq!(got.as_slice(), expected, "offered actions for {needle:?}");
}

#[test]
fn mixed_switch_marks_collapse_preferred() {
    let actions = refactor_actions(
        include_str!("../../../../tests/fixtures/formatter/switch_mixed.ws"),
        "switch",
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
fn collapse_action_carries_the_collapsed_text() {
    let actions = refactor_actions(
        include_str!("../../../../tests/fixtures/formatter/switch_block.ws"),
        "switch",
    );
    assert!(new_text(&actions[0]).contains("case 0: Foo(); break;"));
}

const IF_COLLAPSE: &str = "Collapse if/else to single-line bodies";
const IF_EXPAND: &str = "Expand if/else to block bodies";

#[rstest]
#[case::block_on_if(
    include_str!("../../../../tests/fixtures/formatter/if_block.ws"),
    "if",
    &[IF_COLLAPSE]
)]
#[case::block_on_else(
    include_str!("../../../../tests/fixtures/formatter/if_block.ws"),
    "else",
    &[IF_COLLAPSE]
)]
#[case::inline_only_expand(
    include_str!("../../../../tests/fixtures/formatter/if_inline.ws"),
    "if",
    &[IF_EXPAND]
)]
#[case::mixed_collapse_first(
    include_str!("../../../../tests/fixtures/formatter/if_mixed.ws"),
    "if",
    &[IF_COLLAPSE, IF_EXPAND]
)]
#[case::on_statement(
    include_str!("../../../../tests/fixtures/formatter/if_block.ws"),
    "Foo",
    &[IF_COLLAPSE]
)]
#[case::outside_chain(
    include_str!("../../../../tests/fixtures/formatter/if_block.ws"),
    "function",
    &[]
)]
fn offers_expected_if_actions(#[case] src: &str, #[case] needle: &str, #[case] expected: &[&str]) {
    let actions = refactor_actions(src, needle);
    let title_list = titles(&actions);
    let got: Vec<&str> = title_list.iter().map(String::as_str).collect();
    assert_eq!(got.as_slice(), expected, "offered actions for {needle:?}");
}

#[test]
fn mixed_if_marks_collapse_preferred() {
    let actions = refactor_actions(
        include_str!("../../../../tests/fixtures/formatter/if_mixed.ws"),
        "if",
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
fn collapse_action_carries_the_inlined_text() {
    let actions = refactor_actions(
        include_str!("../../../../tests/fixtures/formatter/if_block.ws"),
        "if",
    );
    assert!(new_text(&actions[0]).contains("if (a) Foo();"));
}
