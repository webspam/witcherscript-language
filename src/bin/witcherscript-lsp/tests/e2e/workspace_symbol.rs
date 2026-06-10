use lsp_types::request::WorkspaceSymbolRequest;
use lsp_types::{WorkspaceSymbolParams, WorkspaceSymbolResponse};

use super::harness::LspClient;

#[tokio::test]
async fn advertises_capability() {
    let client = LspClient::spawn().await;
    assert!(
        client
            .server_capabilities()
            .workspace_symbol_provider
            .is_some(),
        "server must advertise workspace symbol support",
    );
}

#[tokio::test]
async fn finds_builtin_symbol_over_the_wire() {
    let mut client = LspClient::spawn().await;

    let response = client
        .request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
            query: "CActionPoint".to_string(),
            ..Default::default()
        })
        .await
        .expect("workspace symbol response");

    // Untagged WorkspaceSymbolResponse: our Nested payload also decodes as Flat, so accept either.
    let names: Vec<String> = match response {
        WorkspaceSymbolResponse::Flat(items) => items.into_iter().map(|i| i.name).collect(),
        WorkspaceSymbolResponse::Nested(items) => items.into_iter().map(|i| i.name).collect(),
    };
    assert!(
        names.iter().any(|n| n == "CActionPoint"),
        "project-wide search should find the builtin class: {names:?}",
    );
}
