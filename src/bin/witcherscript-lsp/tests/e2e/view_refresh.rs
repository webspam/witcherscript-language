use super::harness::LspClient;

const SEMANTIC_TOKENS_REFRESH: &str = "workspace/semanticTokens/refresh";
const CODE_LENS_REFRESH: &str = "workspace/codeLens/refresh";
const DIAGNOSTIC_REFRESH: &str = "workspace/diagnostic/refresh";
const INLAY_HINT_REFRESH: &str = "workspace/inlayHint/refresh";

#[tokio::test]
async fn indexing_refreshes_every_view_the_client_supports() {
    let mut client = LspClient::spawn_with_view_refresh().await;
    assert!(
        client
            .wait_for_server_requests(&[
                SEMANTIC_TOKENS_REFRESH,
                CODE_LENS_REFRESH,
                DIAGNOSTIC_REFRESH,
                INLAY_HINT_REFRESH,
            ])
            .await,
        "after the initial index the server must refresh every view the client advertised support for"
    );
}
