use lsp_types::Url;
use serde_json::json;

use super::harness::LspClient;

// Guards the CatchUnwindLayer wiring only; tests always unwind, so the profile is guarded by release_profile_does_not_abort_on_panic.
#[tokio::test]
async fn panicking_handler_returns_error_and_server_survives() {
    let mut client = LspClient::spawn().await;

    let resp = client.raw_request("test/panic", json!({})).await;
    assert!(
        resp.get("error").is_some(),
        "a panicking request handler must yield an error response, got {resp}"
    );

    let uri: Url = "file:///after-panic.ws".parse().unwrap();
    client.open(&uri, "class Foo {}\n").await;
    let diags = client.wait_diagnostics(&uri).await;
    assert!(
        diags.is_empty(),
        "server must still answer requests after a handler panic, got {diags:?}"
    );
}
