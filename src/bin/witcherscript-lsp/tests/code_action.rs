use lsp_types::{CodeActionKind, CodeActionOrCommand, Diagnostic, NumberOrString, Range};
use rstest::rstest;
use serde_json::json;

use crate::convert::base_script_conflict_code_actions;

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
