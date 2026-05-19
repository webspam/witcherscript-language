use lsp_types::Url;

use super::harness::LspClient;

#[tokio::test]
async fn diagnostics_emitted_for_unclosed_class() {
    let uri: Url = "file:///bad.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;

    let diags = client.wait_diagnostics(&uri).await;
    assert!(
        !diags.is_empty(),
        "expected at least one diagnostic for unclosed class body"
    );
}

#[tokio::test]
async fn diagnostics_clear_after_fixing_source() {
    let uri: Url = "file:///fix.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, "class Foo {\n").await;
    let bad = client.wait_diagnostics(&uri).await;
    assert!(!bad.is_empty(), "broken source should report diagnostics");

    client.change_full(&uri, 2, "class Foo {}\n").await;
    let good = client.wait_diagnostics(&uri).await;
    assert!(
        good.is_empty(),
        "fixed source should clear diagnostics, got {good:?}"
    );
}
