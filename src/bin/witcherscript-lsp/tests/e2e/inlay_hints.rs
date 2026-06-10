use lsp_types::request::InlayHintRequest;
use lsp_types::{
    InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams, Position, Range,
    TextDocumentIdentifier, Url, WorkDoneProgressParams,
};

use super::harness::LspClient;

fn full_range() -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: u32::MAX,
            character: 0,
        },
    }
}

async fn request_hints(client: &mut LspClient, uri: &Url, range: Range) -> Vec<InlayHint> {
    client
        .request::<InlayHintRequest>(InlayHintParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap_or_default()
}

fn label_str(hint: &InlayHint) -> &str {
    match &hint.label {
        InlayHintLabel::String(s) => s,
        InlayHintLabel::LabelParts(_) => panic!("expected a plain-string inlay hint label"),
    }
}

#[tokio::test]
async fn parameter_name_hints_returned() {
    let mut client = LspClient::spawn().await;
    let uri: Url = "file:///main.ws".parse().unwrap();
    client
        .open(
            &uri,
            "function Foo(target : int) {}\nfunction Bar() { Foo(1); }\n",
        )
        .await;

    let hints = request_hints(&mut client, &uri, full_range()).await;
    assert_eq!(hints.len(), 1, "expected one parameter hint: {hints:?}");
    assert_eq!(label_str(&hints[0]), "target:");
    assert_eq!(hints[0].kind, Some(InlayHintKind::PARAMETER));
}

#[tokio::test]
async fn range_excludes_calls_outside_viewport() {
    let mut client = LspClient::spawn().await;
    let uri: Url = "file:///main.ws".parse().unwrap();
    let text = "function Near(near : int) {}\n\
                function Far(far : int) {}\n\
                function Run() {\n\
                Near(1);\n\
                Far(2);\n\
                }\n";
    client.open(&uri, text).await;

    let far_line = text
        .lines()
        .position(|l| l.contains("Far(2)"))
        .expect("fixture contains the Far call");
    let far_line = u32::try_from(far_line).expect("fixture line fits u32");
    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: far_line,
            character: 0,
        },
    };
    let hints = request_hints(&mut client, &uri, range).await;
    let labels: Vec<&str> = hints.iter().map(label_str).collect();
    assert_eq!(
        labels,
        vec!["near:"],
        "only the in-viewport call should be hinted"
    );
}

#[tokio::test]
async fn redundant_parameter_hint_suppressed() {
    let mut client = LspClient::spawn().await;
    let uri: Url = "file:///main.ws".parse().unwrap();
    client
        .open(
            &uri,
            "function Foo(target : int) {}\nfunction Bar() { var target : int; Foo(target); }\n",
        )
        .await;

    let hints = request_hints(&mut client, &uri, full_range()).await;
    assert!(
        hints.is_empty(),
        "an argument that already spells the parameter name should suppress the hint: {hints:?}"
    );
}
