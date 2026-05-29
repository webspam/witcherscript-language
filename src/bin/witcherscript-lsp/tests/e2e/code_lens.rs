use super::harness::LspClient;

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
