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
async fn returns_remove_unused_quickfix() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "function F(foo : int) {}\n").await;

    let diagnostic = Diagnostic {
        range: Range::new(Position::new(0, 10), Position::new(0, 19)),
        code: Some(NumberOrString::String("unused_symbol".to_string())),
        data: Some(json!({
            "removeRanges": [{
                "start": { "line": 0, "character": 10 },
                "end": { "line": 0, "character": 19 },
            }],
            "noun": "param",
        })),
        ..Diagnostic::default()
    };
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: diagnostic.range,
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
    let CodeActionOrCommand::CodeAction(action) = response.last().expect("at least one action")
    else {
        panic!("expected a CodeAction, got {:?}", response.last());
    };
    assert_eq!(action.title, "Remove unused param");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    let edits = action
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.get(&uri))
        .expect("quickfix carries an edit");
    assert_eq!(edits[0].new_text, "");
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
    assert_eq!(
        response.len(),
        2,
        "expression selection offers both extract actions"
    );
    let CodeActionOrCommand::CodeAction(function_action) = &response[1] else {
        panic!("expected a CodeAction, got {:?}", response[1]);
    };
    assert_eq!(function_action.title, "Extract to function");
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
async fn offers_extract_function_for_statement_selection() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    let source = "function Use(x : int) {}\nfunction F() {\n    Use(1 + 2);\n}\n";
    client.open(&uri, source).await;

    // Selection covering the whole `Use(1 + 2);` statement (line 2, characters 4..15).
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::new(Position::new(2, 4), Position::new(2, 15)),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = client
        .request::<CodeActionRequest>(params)
        .await
        .expect("expected a code action response");
    assert_eq!(response.len(), 1, "statement selection offers extract");
    let CodeActionOrCommand::CodeAction(action) = &response[0] else {
        panic!("expected a CodeAction, got {:?}", response[0]);
    };
    assert_eq!(action.title, "Extract to function");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
    let edits = action
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.values().next())
        .expect("extract carries a WorkspaceEdit");
    assert_eq!(edits.len(), 2, "one insert plus one replace");
    assert_eq!(
        edits[0].new_text,
        "\n\nfunction NewFunction() {\n    Use(1 + 2);\n}"
    );
    assert_eq!(edits[1].new_text, "NewFunction();");
    let command = action
        .command
        .as_ref()
        .expect("extract must trigger rename");
    assert_eq!(command.command, "witcherscript.extractVariable");
    assert_eq!(command.title, "Rename extracted function");
}

#[tokio::test]
async fn offers_extract_method_inside_a_class() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    let source =
        "class C {\n    function M() {\n        Use(1 + 2);\n    }\n}\nfunction Use(x : int) {}\n";
    client.open(&uri, source).await;

    // Selection covering the whole `Use(1 + 2);` statement (line 2, characters 8..19).
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::new(Position::new(2, 8), Position::new(2, 19)),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = client
        .request::<CodeActionRequest>(params)
        .await
        .expect("expected a code action response");
    assert_eq!(
        response.len(),
        2,
        "statement inside a method offers extract to method and to function"
    );
    let CodeActionOrCommand::CodeAction(action) = &response[0] else {
        panic!("expected a CodeAction, got {:?}", response[0]);
    };
    assert_eq!(action.title, "Extract to method");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
    let edits = action
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.values().next())
        .expect("extract carries a WorkspaceEdit");
    assert_eq!(edits.len(), 2, "one insert plus one replace");
    assert_eq!(
        edits[0].new_text,
        "\n\n    private function NewMethod() {\n        Use(1 + 2);\n    }"
    );
    assert_eq!(edits[1].new_text, "NewMethod();");
    let command = action
        .command
        .as_ref()
        .expect("extract must trigger rename");
    assert_eq!(command.command, "witcherscript.extractVariable");
    assert_eq!(command.title, "Rename extracted method");
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
async fn offers_join_declaration_and_assignment() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    let source = "function F() {\n    var x : int;\n    x = 5;\n}\n";
    client.open(&uri, source).await;

    let cursor = Position::new(1, 8);
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
    assert_eq!(response.len(), 1, "a bare declaration offers join only");
    let CodeActionOrCommand::CodeAction(action) = &response[0] else {
        panic!("expected a CodeAction, got {:?}", response[0]);
    };
    assert_eq!(action.title, "Join declaration and assignment");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_REWRITE));
    let edits = action
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.values().next())
        .expect("rewrite carries a WorkspaceEdit");
    let texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(texts.contains(&" = 5"), "unexpected edits: {texts:?}");
}

#[tokio::test]
async fn offers_split_declaration_and_initializer() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    let source = "function F() {\n    var x : int = 5;\n}\n";
    client.open(&uri, source).await;

    let cursor = Position::new(1, 8);
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
    assert_eq!(
        response.len(),
        1,
        "an initialised declaration offers split only"
    );
    let CodeActionOrCommand::CodeAction(action) = &response[0] else {
        panic!("expected a CodeAction, got {:?}", response[0]);
    };
    assert_eq!(action.title, "Split declaration and initializer");
    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_REWRITE));
    let edits = action
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.values().next())
        .expect("rewrite carries a WorkspaceEdit");
    let texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(
        texts.contains(&"\n    x = 5;"),
        "unexpected edits: {texts:?}"
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
