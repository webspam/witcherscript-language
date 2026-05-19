use lsp_types::request::Rename;
use lsp_types::{
    RenameParams, TextDocumentIdentifier, TextDocumentPositionParams, WorkDoneProgressParams,
};

use super::fixture::Fixture;
use super::harness::LspClient;

#[tokio::test]
async fn rename_function_rewrites_declaration_and_callsite() {
    let f = Fixture::parse(concat!(
        "function Foo() {}\n",
        "function Bar() { Fo$0o(); }\n",
    ));

    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }

    let (cursor_uri, pos) = f.cursor();
    let edit = client
        .request::<Rename>(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: cursor_uri.clone(),
                },
                position: pos,
            },
            new_name: "Baz".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("rename response");

    let changes = edit.changes.expect("rename produced changes map");
    let edits = changes
        .get(&cursor_uri)
        .expect("edits exist for the cursor URI");
    assert_eq!(edits.len(), 2, "expected declaration + 1 callsite");
    assert!(
        edits.iter().all(|e| e.new_text == "Baz"),
        "every edit should rewrite to the new name, got {edits:?}"
    );
}
