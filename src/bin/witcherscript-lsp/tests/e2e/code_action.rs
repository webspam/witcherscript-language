use lsp_types::request::CodeActionRequest;
use lsp_types::{
    CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionTriggerKind, Diagnostic, NumberOrString, PartialResultParams, Position, Range,
    TextDocumentIdentifier, Url, WorkDoneProgressParams,
};
use serde_json::json;

use super::harness::LspClient;

#[tokio::test]
async fn advertises_code_action_provider() {
    let client = LspClient::spawn().await;
    assert!(
        client.server_capabilities().code_action_provider.is_some(),
        "server must advertise codeActionProvider",
    );
}

#[tokio::test]
async fn returns_quickfix_for_base_script_conflict() {
    let uri: Url = "file:///mod/scripts/game/r4Player.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class CR4Player {}\n").await;

    let diagnostic = Diagnostic {
        range: Range::default(),
        code: Some(NumberOrString::String("base_script_conflict".to_string())),
        data: Some(json!({ "directory": "D:\\MyMod\\scripts" })),
        ..Diagnostic::default()
    };
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::default(),
        context: CodeActionContext {
            diagnostics: vec![diagnostic],
            ..CodeActionContext::default()
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = client
        .request::<CodeActionRequest>(params)
        .await
        .expect("expected a code action response");
    assert_eq!(response.len(), 1, "expected exactly one quickfix");
    let CodeActionOrCommand::CodeAction(action) = &response[0] else {
        panic!("expected a CodeAction, got {:?}", response[0]);
    };
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

#[tokio::test]
async fn offers_collapse_rewrite_on_a_block_switch() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    let source = "function F() {\n    switch (x) {\n        case 0:\n            Foo();\n            break;\n    }\n}\n";
    client.open(&uri, source).await;

    // Cursor on the `switch` keyword (line 1, character 4).
    let cursor = Position::new(1, 4);
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::new(cursor, cursor),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = client
        .request::<CodeActionRequest>(params)
        .await
        .expect("expected a code action response");
    assert_eq!(response.len(), 1, "block switch offers collapse only");
    let CodeActionOrCommand::CodeAction(action) = &response[0] else {
        panic!("expected a CodeAction, got {:?}", response[0]);
    };
    assert_eq!(action.title, "Collapse switch cases to a single line");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_REWRITE));
    let edits = action
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.values().next())
        .expect("rewrite carries a WorkspaceEdit");
    assert!(
        edits[0].new_text.contains("case 0: Foo(); break;"),
        "unexpected rewrite text: {}",
        edits[0].new_text,
    );
}

#[tokio::test]
async fn offers_extract_variable_for_selection() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    let source = "function Use(x : int) {}\nfunction F() {\n    Use(1 + 2);\n}\n";
    client.open(&uri, source).await;

    // Selection covering `1 + 2` (line 2, characters 8..13).
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::new(Position::new(2, 8), Position::new(2, 13)),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = client
        .request::<CodeActionRequest>(params)
        .await
        .expect("expected a code action response");
    assert_eq!(response.len(), 1, "expression selection offers extract");
    let CodeActionOrCommand::CodeAction(action) = &response[0] else {
        panic!("expected a CodeAction, got {:?}", response[0]);
    };
    assert_eq!(action.title, "Extract to variable");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
    let edit = action
        .edit
        .as_ref()
        .expect("extract carries a WorkspaceEdit");
    let edits = edit
        .changes
        .as_ref()
        .and_then(|c| c.values().next())
        .expect("edit targets one document");
    assert_eq!(edits.len(), 2, "one insert plus one replace");
    assert_eq!(edits[0].new_text, "\n    var x : int = 1 + 2;");
    assert_eq!(edits[1].new_text, "x");
    let command = action
        .command
        .as_ref()
        .expect("extract must trigger rename");
    assert_eq!(command.command, "witcherscript.extractVariable");
    let args = command.arguments.as_ref().unwrap();
    assert_eq!(args[0], json!("file:///main.ws"));
    assert_eq!(
        args[1],
        json!({ "line": 3, "character": 8 }),
        "cursor lands on the new variable name at the usage site",
    );
}

#[tokio::test]
async fn automatic_trigger_suppresses_refactors() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    let source = "function F() {\n    switch (x) {\n        case 0:\n            Foo();\n            break;\n    }\n}\n";
    client.open(&uri, source).await;

    // Cursor on the `switch` keyword, but the editor requested this automatically, not the user
    let cursor = Position::new(1, 4);
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::new(cursor, cursor),
        context: CodeActionContext {
            trigger_kind: Some(CodeActionTriggerKind::AUTOMATIC),
            ..CodeActionContext::default()
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = client.request::<CodeActionRequest>(params).await;
    assert!(
        response.is_none_or(|actions| actions.is_empty()),
        "automatic trigger must not surface layout refactors",
    );
}

#[tokio::test]
async fn offers_collapse_rewrite_on_a_block_if() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    let source = "function F() {\n    if (a) {\n        Foo();\n    }\n    else {\n        Bar();\n    }\n}\n";
    client.open(&uri, source).await;

    // Cursor on the `if` keyword (line 1, character 4).
    let cursor = Position::new(1, 4);
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::new(cursor, cursor),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = client
        .request::<CodeActionRequest>(params)
        .await
        .expect("expected a code action response");
    assert_eq!(response.len(), 1, "block if offers collapse only");
    let CodeActionOrCommand::CodeAction(action) = &response[0] else {
        panic!("expected a CodeAction, got {:?}", response[0]);
    };
    assert_eq!(action.title, "Collapse if/else to single-line bodies");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_REWRITE));
    let edits = action
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.values().next())
        .expect("rewrite carries a WorkspaceEdit");
    assert!(
        edits[0].new_text.contains("if (a) Foo();"),
        "unexpected rewrite text: {}",
        edits[0].new_text,
    );
}
