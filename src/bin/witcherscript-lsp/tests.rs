mod code_action;
mod completion;
mod diagnostics;
mod document_highlight;
mod e2e;
mod file_io;
mod file_scope;
mod hover;
mod indexing;
mod inlay_hints;
mod jsonrpc_client;
mod refactoring;

#[test]
fn release_profile_does_not_abort_on_panic() {
    let aborts = include_str!("../../../Cargo.toml").lines().any(|line| {
        let code = line.split('#').next().unwrap_or("");
        code.replace(' ', "").contains(r#"panic="abort""#)
    });
    assert!(
        !aborts,
        "release profile must not abort: the LSP server needs unwinding so \
         async-lsp's CatchUnwindLayer keeps it alive after a handler panic"
    );
}
