use async_lsp::ErrorCode;
use serde_json::{Value, json};
use witcherscript_language::builtins::{BUILTIN_ARRAY_URI, builtin_source};

use super::harness::LspClient;

const BUILTIN_SOURCE_METHOD: &str = "witcherscript/builtinSource";

#[tokio::test]
async fn serves_embedded_source_for_a_known_builtin_uri() {
    let mut client = LspClient::spawn().await;
    let response = client
        .raw_request(BUILTIN_SOURCE_METHOD, json!({ "uri": BUILTIN_ARRAY_URI }))
        .await;
    let text = response
        .pointer("/result/text")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("known builtin URI must return a text field, got {response}"));
    assert_eq!(
        text,
        builtin_source(BUILTIN_ARRAY_URI).expect("array builtin exists"),
        "served text must equal the embedded builtin source",
    );
}

#[tokio::test]
async fn resolves_an_unknown_builtin_uri_to_null() {
    let mut client = LspClient::spawn().await;
    let response = client
        .raw_request(
            BUILTIN_SOURCE_METHOD,
            json!({ "uri": "witcherscript-builtin:/does-not-exist.ws" }),
        )
        .await;
    assert_eq!(
        response.get("result"),
        Some(&Value::Null),
        "an unknown builtin URI must resolve to null, got {response}",
    );
}

#[tokio::test]
async fn rejects_a_missing_uri_parameter() {
    let mut client = LspClient::spawn().await;
    let response = client.raw_request(BUILTIN_SOURCE_METHOD, json!({})).await;
    let code = response
        .pointer("/error/code")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing uri must produce an error, got {response}"));
    assert_eq!(
        code,
        i64::from(ErrorCode::INVALID_PARAMS.0),
        "missing uri must be rejected as invalid params, got {response}",
    );
}
