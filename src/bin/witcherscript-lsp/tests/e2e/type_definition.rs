use lsp_types::request::GotoTypeDefinition;
use lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, PartialResultParams, Position,
    TextDocumentIdentifier, TextDocumentPositionParams, Url, WorkDoneProgressParams,
};

use super::fixture::Fixture;
use super::harness::LspClient;

async fn goto_type(
    client: &mut LspClient,
    uri: Url,
    pos: Position,
) -> Option<GotoDefinitionResponse> {
    client
        .request::<GotoTypeDefinition>(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
}

async fn assert_jumps_to(fixture_text: &str, label: &str) {
    let f = Fixture::parse(fixture_text);
    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }
    let (cursor_uri, pos) = f.cursor();
    let resp = goto_type(&mut client, cursor_uri, pos)
        .await
        .expect("type definition resolved");

    let (expected_uri, expected_range) = f.span(label);
    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.uri, expected_uri);
            assert_eq!(loc.range, expected_range);
        }
        other => panic!("expected scalar response, got {other:?}"),
    }
}

#[tokio::test]
async fn type_definition_for_var_of_class() {
    assert_jumps_to(
        concat!(
            "class CExample {}\n",
            "//    ^^^^^^^^ target\n",
            "function Test() {\n",
            "    var x$0 : CExample;\n",
            "}\n",
        ),
        "target",
    )
    .await;
}

#[tokio::test]
async fn type_definition_for_var_of_array_strips_generic() {
    let f = Fixture::parse(concat!(
        "class CExample {}\n",
        "function Test() {\n",
        "    var ar$0r : array<CExample>;\n",
        "}\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }
    let (cursor_uri, pos) = f.cursor();
    let resp = goto_type(&mut client, cursor_uri, pos)
        .await
        .expect("type definition resolved");

    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert!(
                loc.uri.as_str().contains("array"),
                "array<T> should resolve to the array builtin, got {}",
                loc.uri
            );
        }
        other => panic!("expected scalar response, got {other:?}"),
    }
}

#[tokio::test]
async fn type_definition_for_function_return_type() {
    assert_jumps_to(
        concat!(
            "class CExample {}\n",
            "//    ^^^^^^^^ target\n",
            "function f$0() : CExample { return new CExample in this; }\n",
        ),
        "target",
    )
    .await;
}

#[tokio::test]
async fn type_definition_on_class_itself_returns_self() {
    assert_jumps_to(
        concat!("class CExa$0mple {}\n", "//    ^^^^^^^^ target\n",),
        "target",
    )
    .await;
}

#[tokio::test]
async fn type_definition_on_enum_member_jumps_to_owning_enum() {
    assert_jumps_to(
        concat!(
            "enum EFoo { EFooAlpha, EFooBeta }\n",
            "//   ^^^^ target\n",
            "function Test() {\n",
            "    var x : EFoo = EFooA$0lpha;\n",
            "}\n",
        ),
        "target",
    )
    .await;
}

#[tokio::test]
async fn type_definition_for_primitive_returns_none() {
    let f = Fixture::parse(concat!(
        "function Test() {\n",
        "    var x$0 : int;\n",
        "}\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }
    let (cursor_uri, pos) = f.cursor();
    let resp = goto_type(&mut client, cursor_uri, pos).await;

    assert!(resp.is_none(), "expected None for primitive type");
}
