use lsp_types::request::CodeActionRequest;
use lsp_types::{
    CodeActionContext, CodeActionOrCommand, CodeActionParams, Diagnostic, NumberOrString,
    PartialResultParams, Range, TextDocumentIdentifier, Url, WorkDoneProgressParams,
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
