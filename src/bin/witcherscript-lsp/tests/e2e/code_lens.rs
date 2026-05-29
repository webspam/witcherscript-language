use super::harness::LspClient;

#[tokio::test]
async fn advertises_code_lens_provider() {
    let client = LspClient::spawn().await;
    assert!(
        client.server_capabilities().code_lens_provider.is_some(),
        "server must advertise codeLensProvider",
    );
}
