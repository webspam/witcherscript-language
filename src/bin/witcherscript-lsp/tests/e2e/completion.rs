use lsp_types::request::{Completion, ResolveCompletionItem};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    CompletionTriggerKind, Documentation, PartialResultParams, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams,
};

use super::fixture::Fixture;
use super::harness::LspClient;

fn items_of(resp: CompletionResponse) -> Vec<CompletionItem> {
    match resp {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    }
}

fn trigger_context(trigger_character: &str) -> CompletionContext {
    CompletionContext {
        trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
        trigger_character: Some(trigger_character.to_string()),
    }
}

#[tokio::test]
async fn typing_at_sign_completes_annotation_without_doubling_it() {
    let uri: lsp_types::Url = "file:///main.ws".parse().unwrap();
    let at = lsp_types::Position {
        line: 0,
        character: 0,
    };
    let mut client = LspClient::spawn().await;
    client.open(&uri, "").await;
    client
        .notify::<lsp_types::notification::DidChangeTextDocument>(
            lsp_types::DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![lsp_types::TextDocumentContentChangeEvent {
                    range: Some(lsp_types::Range { start: at, end: at }),
                    range_length: None,
                    text: "@".to_string(),
                }],
            },
        )
        .await;
    let resp = client
        .request::<Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: lsp_types::Position {
                    line: 0,
                    character: 1,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(trigger_context("@")),
        })
        .await
        .expect("completion response");

    let item = items_of(resp)
        .into_iter()
        .find(|i| i.label == "@addField")
        .expect("@addField offered when typing @");
    assert!(
        item.text_edit.is_none(),
        "must not use a replace-range that deletes the typed @, got {:?}",
        item.text_edit
    );
    assert_eq!(
        item.insert_text.as_deref(),
        Some("addField($1)"),
        "insert text must exclude the leading @, else the typed @ doubles it"
    );
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
async fn resolve_fills_documentation_without_touching_eager_fields() {
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

    assert_eq!(
        client
            .server_capabilities()
            .completion_provider
            .as_ref()
            .and_then(|p| p.resolve_provider),
        Some(true),
        "server must advertise completionItem/resolve support",
    );

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
    assert!(
        method.documentation.is_none(),
        "list items must not carry documentation eagerly"
    );
    assert!(method.data.is_some(), "list items must carry resolve data");

    let resolved = client
        .request::<ResolveCompletionItem>(method.clone())
        .await;
    let Some(Documentation::MarkupContent(markup)) = &resolved.documentation else {
        panic!(
            "resolve must fill markdown documentation, got {:?}",
            resolved.documentation
        );
    };
    assert!(
        markup.value.contains("(method) CExample.DoThing() : void"),
        "documentation must carry the signature, got {:?}",
        markup.value
    );
    assert_eq!(resolved.label, method.label, "label must not change");
    assert_eq!(
        resolved.insert_text, method.insert_text,
        "insertText must not change during resolve"
    );
    assert_eq!(
        resolved.sort_text, method.sort_text,
        "sortText must not change during resolve"
    );
    assert_eq!(
        resolved.detail, method.detail,
        "detail must not change during resolve"
    );
}

#[tokio::test]
async fn dot_trigger_in_member_access_returns_members() {
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
            context: Some(trigger_context(".")),
        })
        .await
        .expect("completion response");

    let labels: Vec<String> = items_of(resp).into_iter().map(|i| i.label).collect();
    assert!(
        labels.contains(&"DoThing".to_string()),
        "dot trigger on member access should still list members, got {labels:?}"
    );
}

#[tokio::test]
async fn no_trigger_offers_completions_in_a_comment() {
    let f = Fixture::parse(concat!(
        "function Test() {\n",
        "    // pick up the loot$0\n",
        "}\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let contexts = [
        Some(trigger_context(".")),
        Some(trigger_context(":")),
        Some(trigger_context("@")),
        None,
    ];
    for context in contexts {
        let resp = client
            .request::<Completion>(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: cursor_uri.clone(),
                    },
                    position: pos,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: context.clone(),
            })
            .await;

        assert!(
            resp.is_none(),
            "no completions expected in a comment (context {context:?}), got {resp:?}"
        );
    }
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
async fn completion_after_wrap_method_offers_methods_with_function_keyword() {
    let f = Fixture::parse(concat!(
        "class CPlayer {\n",
        "    public function OnSpawned() : void {}\n",
        "}\n",
        "@wrapMethod(CPlayer) $0\n",
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

    let method = items_of(resp)
        .into_iter()
        .find(|i| i.label == "OnSpawned")
        .expect("OnSpawned offered directly after @wrapMethod");
    assert_eq!(method.kind, Some(CompletionItemKind::METHOD));
    assert!(
        method
            .insert_text
            .as_deref()
            .is_some_and(|t| t.starts_with("function OnSpawned(")),
        "insert must lead with the `function` keyword, got {:?}",
        method.insert_text
    );
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
