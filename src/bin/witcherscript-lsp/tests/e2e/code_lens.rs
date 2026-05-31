use lsp_types::Url;

use super::harness::LspClient;

const CODE_LENS_REFRESH: &str = "workspace/codeLens/refresh";

#[tokio::test]
async fn opening_and_closing_a_file_refreshes_code_lenses() {
    let mut client = LspClient::spawn_with_code_lens_refresh().await;
    // The startup refresh fires once index_base_scripts settles; consume it first to isolate the rest.
    assert!(
        client.wait_for_server_request(CODE_LENS_REFRESH).await,
        "server refreshes code lenses after the initial index"
    );

    let uri = Url::parse("file:///e2e_refresh.ws").expect("uri parses");
    client.open(&uri, "function Foo() {}\n").await;
    assert!(
        client.wait_for_server_request(CODE_LENS_REFRESH).await,
        "opening a file changes the indexed set, so counts in other files must be recomputed"
    );

    client.close(&uri).await;
    assert!(
        client.wait_for_server_request(CODE_LENS_REFRESH).await,
        "closing a file reverts its index contribution, so counts must be recomputed"
    );
}

#[tokio::test]
async fn advertises_code_lens_provider() {
    let client = LspClient::spawn().await;
    let provider = client
        .server_capabilities()
        .code_lens_provider
        .expect("server must advertise codeLensProvider");
    assert_eq!(
        provider.resolve_provider,
        Some(true),
        "the references lens needs lazy codeLens/resolve",
    );
}
