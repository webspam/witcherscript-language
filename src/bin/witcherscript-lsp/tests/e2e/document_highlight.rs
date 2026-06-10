use lsp_types::request::DocumentHighlightRequest;
use lsp_types::{
    DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams, PartialResultParams,
    Position, TextDocumentIdentifier, TextDocumentPositionParams, Url, WorkDoneProgressParams,
};

use super::fixture::Fixture;
use super::harness::LspClient;

async fn highlights(
    client: &mut LspClient,
    uri: Url,
    pos: Position,
) -> Option<Vec<DocumentHighlight>> {
    client
        .request::<DocumentHighlightRequest>(DocumentHighlightParams {
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
async fn document_highlight_returns_read_and_write_kinds() {
    let f = Fixture::parse(concat!(
        "function F() {\n",
        "    var count : int;\n",
        "//      ^^^^^ decl\n",
        "    $0count = 1;\n",
        "//  ^^^^^ write\n",
        "    Use(count);\n",
        "//      ^^^^^ read\n",
        "}\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let hits = highlights(&mut client, cursor_uri, pos)
        .await
        .expect("highlights returned");

    let expect = |label: &str, kind: DocumentHighlightKind| {
        let (_, range) = f.span(label);
        let found = hits
            .iter()
            .any(|h| h.range == range && h.kind == Some(kind));
        assert!(
            found,
            "missing {label} highlight {range:?} as {kind:?}: {hits:?}"
        );
    };
    expect("decl", DocumentHighlightKind::WRITE);
    expect("write", DocumentHighlightKind::WRITE);
    expect("read", DocumentHighlightKind::READ);
    assert_eq!(hits.len(), 3, "exactly three occurrences: {hits:?}");
}

#[tokio::test]
async fn document_highlight_returns_none_at_whitespace() {
    let mut client = LspClient::spawn().await;
    let uri: Url = "file:///main.ws".parse().unwrap();
    client.open(&uri, "function Foo() {}\n").await;

    let resp = highlights(
        &mut client,
        uri,
        Position {
            line: 0,
            character: 0,
        },
    )
    .await;

    assert!(resp.is_none(), "expected no highlights at whitespace");
}
