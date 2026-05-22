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

#[tokio::test]
async fn closing_a_file_in_open_files_scope_clears_its_diagnostics() {
    let uri: Url = "file:///scoped.ws".parse().unwrap();
    let mut client = LspClient::spawn_open_files_scope().await;
    client.open(&uri, "class Foo {\n").await;
    let diags = client.wait_diagnostics(&uri).await;
    assert!(
        !diags.is_empty(),
        "open broken file should report diagnostics"
    );

    client.close(&uri).await;
    let cleared = client.wait_diagnostics(&uri).await;
    assert!(
        cleared.is_empty(),
        "closing in open-files scope must clear diagnostics, got {cleared:?}"
    );
}
