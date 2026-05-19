use lsp_types::request::HoverRequest;
use lsp_types::{
    HoverContents, HoverParams, MarkupKind, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams,
};

use super::fixture::Fixture;
use super::harness::LspClient;

#[tokio::test]
async fn hover_on_function_callsite_returns_signature_markdown() {
    let f = Fixture::parse(concat!(
        "function Foo() : void {}\n",
        "function Bar() { Fo$0o(); }\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let hover = client
        .request::<HoverRequest>(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: cursor_uri },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("hover response");

    let HoverContents::Markup(markup) = hover.contents else {
        panic!("expected markup hover contents");
    };
    assert_eq!(markup.kind, MarkupKind::Markdown);
    assert!(
        markup.value.contains("function Foo"),
        "hover markdown missing function signature: {:?}",
        markup.value
    );
}

#[tokio::test]
async fn hover_returns_none_at_whitespace() {
    let mut client = LspClient::spawn().await;
    let uri: lsp_types::Url = "file:///main.ws".parse().unwrap();
    client.open(&uri, "function Foo() {}\n").await;

    let hover = client
        .request::<HoverRequest>(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: lsp_types::Position {
                    line: 0,
                    character: 0,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await;

    assert!(hover.is_none(), "expected no hover at whitespace");
}
