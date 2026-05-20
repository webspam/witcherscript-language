use lsp_types::Url;

use super::harness::LspClient;

#[tokio::test]
async fn did_save_does_not_crash_server() {
    let uri: Url = "file:///save.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;
    let diags = client.wait_diagnostics(&uri).await;
    assert!(!diags.is_empty(), "broken source should report diagnostics");

    client.did_save(&uri).await;
    client.did_save(&uri).await;

    client.change_full(&uri, 2, "class Foo {}\n").await;
    let diags = client.wait_diagnostics(&uri).await;
    assert!(
        diags.is_empty(),
        "server should still respond after didSave, got {diags:?}"
    );
}
