use lsp_types::request::Completion;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, PartialResultParams,
    TextDocumentIdentifier, TextDocumentPositionParams, WorkDoneProgressParams,
};

use super::fixture::Fixture;
use super::harness::LspClient;

fn items_of(resp: CompletionResponse) -> Vec<CompletionItem> {
    match resp {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    }
}

#[tokio::test]
async fn completion_after_dot_returns_class_method() {
    let f = Fixture::parse(concat!(
        "class CExample {\n",
        "    public function DoThing() : void {}\n",
        "}\n",
        "function Test() {\n",
        "    var e : CExample;\n",
        "    e.$0\n",
        "}\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let resp = client
        .request::<Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: cursor_uri },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .expect("completion response");

    let items = items_of(resp);
    let method = items
        .iter()
        .find(|i| i.label == "DoThing")
        .expect("DoThing completion item present");
    assert_eq!(method.kind, Some(CompletionItemKind::METHOD));
}

#[tokio::test]
async fn completion_after_new_keyword_offers_classes() {
    let f = Fixture::parse(concat!(
        "class CBase {}\n",
        "class CDerived extends CBase {}\n",
        "class CUnrelated {}\n",
        "function Test() {\n",
        "    var x : CBase = new $0;\n",
        "}\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let resp = client
        .request::<Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: cursor_uri },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .expect("completion response");

    let items = items_of(resp);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"CBase"), "got {labels:?}");
    assert!(labels.contains(&"CDerived"), "got {labels:?}");
    assert!(!labels.contains(&"CUnrelated"), "got {labels:?}");
}

#[tokio::test]
async fn completion_after_new_lifetime_in_offers_class_locals() {
    let f = Fixture::parse(concat!(
        "class CObject {}\n",
        "class CHolder {}\n",
        "function Test() {\n",
        "    var owner : CHolder;\n",
        "    var n : int;\n",
        "    var x : CObject = new CObject in $0;\n",
        "}\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let resp = client
        .request::<Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: cursor_uri },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .expect("completion response");

    let items = items_of(resp);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"owner"), "got {labels:?}");
    assert!(!labels.contains(&"n"), "got {labels:?}");
}

#[tokio::test]
async fn completion_in_statement_offers_keywords() {
    let f = Fixture::parse(concat!("function Test() {\n", "    $0\n", "}\n",));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let resp = client
        .request::<Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: cursor_uri },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .expect("completion response");

    let items = items_of(resp);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    for kw in ["var", "if", "return"] {
        assert!(
            labels.contains(&kw),
            "expected {kw:?} in statement completions, got {labels:?}"
        );
    }
}
