use lsp_types::request::GotoDefinition;
use lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, PartialResultParams, Position,
    TextDocumentIdentifier, TextDocumentPositionParams, Url, WorkDoneProgressParams,
};

use super::fixture::Fixture;
use super::harness::LspClient;

async fn goto(client: &mut LspClient, uri: Url, pos: Position) -> Option<GotoDefinitionResponse> {
    client
        .request::<GotoDefinition>(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
}

#[tokio::test]
async fn definition_resolves_function_callsite_to_declaration() {
    let f = Fixture::parse(concat!(
        "function Foo() {}\n",
        "//       ^^^ name\n",
        "function Bar() { Fo$0o(); }\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let resp = goto(&mut client, cursor_uri, pos)
        .await
        .expect("definition resolved");

    let (expected_uri, expected_range) = f.span("name");
    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.uri, expected_uri);
            assert_eq!(loc.range, expected_range);
        }
        other => panic!("expected scalar response, got {other:?}"),
    }
}

#[tokio::test]
async fn definition_resolves_class_member_via_dot() {
    let f = Fixture::parse(concat!(
        "class CExample {\n",
        "    public function DoThing() : void {}\n",
        "//                  ^^^^^^^ method\n",
        "}\n",
        "function Test() {\n",
        "    var e : CExample;\n",
        "    e.DoTh$0ing();\n",
        "}\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let resp = goto(&mut client, cursor_uri, pos)
        .await
        .expect("definition resolved");

    let (expected_uri, expected_range) = f.span("method");
    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.uri, expected_uri);
            assert_eq!(loc.range, expected_range);
        }
        other => panic!("expected scalar response, got {other:?}"),
    }
}

#[tokio::test]
async fn definition_returns_none_at_whitespace() {
    let mut client = LspClient::spawn().await;
    let uri: Url = "file:///main.ws".parse().unwrap();
    client.open(&uri, "function Foo() {}\n").await;

    let resp = goto(
        &mut client,
        uri,
        Position {
            line: 0,
            character: 0,
        },
    )
    .await;

    assert!(resp.is_none(), "expected no definition at whitespace");
}
