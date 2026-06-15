use lsp_types::{CodeActionKind, CodeActionOrCommand, Diagnostic, NumberOrString, Range, Url};
use rstest::rstest;
use serde_json::json;
use witcherscript_language::formatter::FormatOptions;
use witcherscript_language::test_support::TestDb;

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
    assert_eq!(action.diagnostics.as_ref().map(std::vec::Vec::len), Some(1));
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
        action.diagnostics.as_ref().map(std::vec::Vec::len),
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
    let cursor = src.find(needle).expect("needle present") + 1;
    refactor_actions_for_range(src, cursor..cursor)
}

fn refactor_actions_for_selection(src: &str, needle: &str) -> Vec<CodeActionOrCommand> {
    let start = src.find(needle).expect("needle present");
    refactor_actions_for_range(src, start..start + needle.len())
}

fn refactor_actions_for_range(
    src: &str,
    range: std::ops::Range<usize>,
) -> Vec<CodeActionOrCommand> {
    let t = TestDb::new(src);
    let uri_str = t.primary_uri();
    let doc = t.doc_for(uri_str);
    let options = FormatOptions::default();
    let uri = Url::parse(uri_str).unwrap();
    refactor_code_actions(&uri, uri_str, doc, &t.db(), range, options)
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

const EXTRACT_SRC: &str = "function Use(x : int) {}\nfunction F() {\n    Use(1 + 2);\n}\n";

#[test]
fn offers_extract_for_exact_expression_selection() {
    let actions = refactor_actions_for_selection(EXTRACT_SRC, "1 + 2");
    assert_eq!(
        titles(&actions),
        vec!["Extract to variable", "Extract to function"]
    );
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
    let edits = extract_workspace_edit(action);
    assert_eq!(edits.len(), 2, "one insert plus one replace");
    assert_eq!(edits[0].new_text, "\n    var x : int = 1 + 2;");
    assert_eq!(edits[1].new_text, "x");
}

fn extract_workspace_edit(action: &lsp_types::CodeAction) -> Vec<lsp_types::TextEdit> {
    action
        .edit
        .clone()
        .and_then(|e| e.changes)
        .and_then(|mut c| c.drain().next())
        .expect("extract carries a WorkspaceEdit")
        .1
}

#[test]
fn extract_action_carries_rename_command() {
    let actions = refactor_actions_for_selection(EXTRACT_SRC, "1 + 2");
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    let command = action
        .command
        .as_ref()
        .expect("extract must trigger rename");
    assert_eq!(command.command, "witcherscript.extractVariable");
    let args = command.arguments.as_ref().unwrap();
    assert_eq!(args[0], json!("file:///main.ws"));
    // The original selection's left-most byte, now the new var name `x` in `Use(x)`.
    assert_eq!(args[1], json!({ "line": 3, "character": 8 }));
}

#[test]
fn extract_function_offered_for_expression_selection() {
    let actions = refactor_actions_for_selection(EXTRACT_SRC, "1 + 2");
    let action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(action) if action.title == "Extract to function" => {
                Some(action)
            }
            _ => None,
        })
        .expect("expression selection offers extract to function");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
    let edits = extract_workspace_edit(action);
    assert_eq!(edits.len(), 2, "one insert plus one replace");
    assert_eq!(
        edits[0].new_text,
        "\n\nfunction NewFunction() : int {\n    return 1 + 2;\n}"
    );
    assert_eq!(edits[1].new_text, "NewFunction()");
}

#[test]
fn statement_selection_offers_extract_function_only() {
    let actions = refactor_actions_for_selection(EXTRACT_SRC, "Use(1 + 2);");
    assert_eq!(titles(&actions), vec!["Extract to function"]);
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    let edits = extract_workspace_edit(action);
    assert_eq!(
        edits[0].new_text,
        "\n\nfunction NewFunction() {\n    Use(1 + 2);\n}"
    );
    assert_eq!(edits[1].new_text, "NewFunction();");
}

#[test]
fn extract_function_reuses_the_extract_command() {
    let actions = refactor_actions_for_selection(EXTRACT_SRC, "Use(1 + 2);");
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    let command = action
        .command
        .as_ref()
        .expect("extract must trigger rename");
    assert_eq!(command.command, "witcherscript.extractVariable");
    assert_eq!(command.title, "Rename extracted function");
    let args = command.arguments.as_ref().unwrap();
    // The call replaces the statement in place, so the new name starts at the same position.
    assert_eq!(args[1], json!({ "line": 2, "character": 4 }));
}

#[test]
fn no_extract_action_for_caret_only_request() {
    let actions = refactor_actions(EXTRACT_SRC, "1 + 2");
    assert!(
        actions.is_empty(),
        "caret without selection must not offer extract, got {actions:?}"
    );
}

const CLASS_EXTRACT_SRC: &str = "class C {\n    var hp : int;\n    function M() {\n        var r : int;\n        r = hp + 2;\n    }\n}\n";

#[test]
fn expression_in_a_class_offers_method_between_variable_and_function() {
    let actions = refactor_actions_for_selection(CLASS_EXTRACT_SRC, "hp + 2");
    assert_eq!(
        titles(&actions),
        vec![
            "Extract to variable",
            "Extract to method",
            "Extract to function"
        ]
    );
}

#[test]
fn extract_method_inserts_a_private_sibling_method() {
    let actions = refactor_actions_for_selection(CLASS_EXTRACT_SRC, "hp + 2");
    let action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(action) if action.title == "Extract to method" => {
                Some(action)
            }
            _ => None,
        })
        .expect("expression selection in a class offers extract to method");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
    let edits = extract_workspace_edit(action);
    assert_eq!(edits.len(), 2, "one insert plus one replace");
    assert_eq!(
        edits[0].new_text,
        "\n\n    private function NewMethod() : int {\n        return hp + 2;\n    }"
    );
    assert_eq!(edits[1].new_text, "NewMethod()");
    let command = action
        .command
        .as_ref()
        .expect("extract must trigger rename");
    assert_eq!(command.title, "Rename extracted method");
}

#[test]
fn statement_in_a_class_offers_method_and_function() {
    let src =
        "function Use(x : int) {}\nclass C {\n    function M() {\n        Use(1 + 2);\n    }\n}\n";
    let actions = refactor_actions_for_selection(src, "Use(1 + 2);");
    assert_eq!(
        titles(&actions),
        vec!["Extract to method", "Extract to function"]
    );
}

// A write before the selection forces the split form: uninitialised decl at the top, assignment in place.
const EXTRACT_SPLIT_SRC: &str =
    "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 2;\n    Use(a + 1);\n}\n";

#[test]
fn split_extract_emits_decl_assignment_and_replacement() {
    let actions = refactor_actions_for_selection(EXTRACT_SPLIT_SRC, "a + 1");
    // `a` is a single-assignment local, so inlining it is also offered here.
    assert_eq!(
        titles(&actions),
        vec![
            "Extract to variable",
            "Extract to function",
            "Inline variable"
        ]
    );
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    let edits = extract_workspace_edit(action);
    assert_eq!(
        edits.len(),
        3,
        "decl plus in-place assignment plus replacement"
    );
    assert_eq!(edits[0].new_text, "\n    var x : int;");
    assert_eq!(edits[1].new_text, "x = a + 1;\n    ");
    assert_eq!(edits[2].new_text, "x");
}

#[test]
fn split_extract_places_rename_on_the_use_site() {
    let actions = refactor_actions_for_selection(EXTRACT_SPLIT_SRC, "a + 1");
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    let command = action
        .command
        .as_ref()
        .expect("extract must trigger rename");
    let args = command.arguments.as_ref().unwrap();
    // After the split's two inserts, the original `a + 1` is the new `x` in `Use(x)`.
    assert_eq!(args[1], json!({ "line": 6, "character": 8 }));
}

const INLINE_SRC: &str = "function F() {\n    var count : int = 5;\n    Foo(count);\n}\n";

#[test]
fn offers_inline_variable_on_declaration() {
    let actions = refactor_actions(INLINE_SRC, "count : int");
    assert_eq!(titles(&actions), vec!["Inline variable"]);
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_INLINE));
    let edits = extract_workspace_edit(action);
    assert_eq!(
        edits.len(),
        2,
        "one replacement plus the declaration deletion"
    );
    let texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(
        texts.contains(&"5"),
        "the use is replaced by the initializer"
    );
    assert!(texts.contains(&""), "the declaration is deleted");
}

#[test]
fn offers_inline_single_usage_on_a_use() {
    let src = "function F() {\n    var count : int = 5;\n    Foo(count);\n    Bar(count);\n}\n";
    let actions = refactor_actions(src, "count);");
    assert_eq!(titles(&actions), vec!["Inline variable"]);
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_INLINE));
    let edits = extract_workspace_edit(action);
    assert_eq!(edits.len(), 1, "only the single occurrence is replaced");
    assert_eq!(edits[0].new_text, "5");
}

#[test]
fn offers_inline_for_dead_initializer() {
    let src = "function F() {\n    var rah : int = 0;\n    rah = 14;\n    if (true) {\n        return rah;\n    }\n}\n";
    let actions = refactor_actions(src, "rah : int");
    assert_eq!(titles(&actions), vec!["Inline variable"]);
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_INLINE));
    let edits = extract_workspace_edit(action);
    assert_eq!(
        edits.len(),
        3,
        "the read is replaced and both the declaration and the reassignment are removed"
    );
    let texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(
        texts.contains(&"14"),
        "the read takes the reaching assignment's value"
    );
    assert_eq!(
        texts.iter().filter(|t| t.is_empty()).count(),
        2,
        "the declaration and the dead initializer's reassignment are both deleted"
    );
}

#[test]
fn flags_inline_when_value_is_unverified() {
    let src = "function F() {\n    var a : int = 1;\n    var x : int = 0;\n    x = a;\n    a = 99;\n    return x;\n}\n";
    let actions = refactor_actions(src, "x;");
    assert_eq!(titles(&actions), vec!["Inline variable (unverified)"]);
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_INLINE));
    assert!(
        !extract_workspace_edit(action).is_empty(),
        "a flagged inline is still applicable and carries edits"
    );
}

#[test]
fn inline_on_declaration_with_many_uses_says_all() {
    let src = "function F() {\n    var count : int = 5;\n    Foo(count);\n    Bar(count);\n}\n";
    let actions = refactor_actions(src, "count : int");
    assert_eq!(titles(&actions), vec!["Inline variable (all)"]);
}

#[test]
fn inlining_the_last_use_deletes_the_declaration() {
    let actions = refactor_actions(INLINE_SRC, "count);");
    assert_eq!(titles(&actions), vec!["Inline variable"]);
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, got {:?}", actions[0]);
    };
    let edits = extract_workspace_edit(action);
    assert_eq!(
        edits.len(),
        2,
        "the last use is replaced and the now-dead declaration removed"
    );
    let texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(texts.contains(&"5"), "the use is replaced by the value");
    assert!(texts.contains(&""), "the declaration is deleted");
}
