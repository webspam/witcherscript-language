use std::fs;

use lsp_types::request::References;
use lsp_types::{
    Location, PartialResultParams, Position, ReferenceContext, ReferenceParams,
    TextDocumentIdentifier, TextDocumentPositionParams, Url, WorkDoneProgressParams,
};

use super::harness::LspClientBuilder;
use crate::tests::support::LocalTempDir;

// Regression test: closing a file does not break a later references request.
#[tokio::test]
async fn references_into_a_reverted_closed_file_resolves() {
    let tmp = LocalTempDir::under_target("close-drift-references");
    let scripts = tmp.path().join("ws").join("scripts");
    fs::create_dir_all(&scripts).expect("mkdir ws");

    let def_path = scripts.join("shared.ws");
    let ref_path = scripts.join("user.ws");
    fs::write(&def_path, "class CShared {}\n").expect("write def file");
    fs::write(&ref_path, "function f() {}\n").expect("write ref file");

    let mut client = LspClientBuilder::new()
        .root(&tmp.path().join("ws"))
        .spawn()
        .await;

    let def_uri = Url::from_file_path(&def_path).expect("def path -> uri");
    let ref_uri = Url::from_file_path(&ref_path).expect("ref path -> uri");

    // CShared sits past the short version's end, so its offset is out of bounds there.
    let long = "function f() {\n  var s : CShared;\n}\n";
    client.open(&ref_uri, long).await;
    fs::write(&ref_path, long).expect("disk catches up to the buffer");
    client.close(&ref_uri).await;

    client.open(&def_uri, "class CShared {}\n").await;
    let result: Option<Vec<Location>> = client
        .request_when_ready::<References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: def_uri.clone(),
                },
                position: Position {
                    line: 0,
                    character: 6,
                },
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await;

    let locations = result.expect("references must return a result");
    assert!(
        locations.iter().any(|loc| loc.uri == ref_uri),
        "references must find the use inside the reverted file, got {locations:?}",
    );
}
