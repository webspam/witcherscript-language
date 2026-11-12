use std::sync::Arc;
use std::sync::atomic::Ordering;

use lsp_types::{
    DidChangeTextDocumentParams, TextDocumentContentChangeEvent, Url,
    VersionedTextDocumentIdentifier,
};
use witcherscript_language::document::{ParsedDocument, parse_document};

use crate::backend::Backend;
use crate::tests::support::{change_params, make_backend, open_params};

fn full_replace_params(uri: &Url, version: i32, text: &str) -> DidChangeTextDocumentParams {
    DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text.to_string(),
        }],
    }
}

fn published_document(backend: &Backend, uri: &Url) -> Arc<ParsedDocument> {
    backend
        .snapshot()
        .documents
        .get(uri)
        .cloned()
        .expect("document published")
}

#[test]
fn publishing_an_older_parse_keeps_the_newer_document() {
    let backend = make_backend();
    let uri: Url = "file:///ordering.ws".parse().unwrap();
    let older = Arc::new(parse_document("function Old() {}\n").expect("parse older"));
    let newer = Arc::new(parse_document("function New() {}\n").expect("parse newer"));

    backend.publish_open_document_indices(&uri, &newer);
    backend.publish_open_document_indices(&uri, &older);

    assert_eq!(
        published_document(&backend, &uri).source,
        newer.source,
        "an older parse landing late must not overwrite the newer published document",
    );
}

#[test]
fn full_replace_drops_the_queued_edit_it_supersedes() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);
    let uri: Url = "file:///full_replace.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "function Foo() {}\n"));
    backend._did_change(change_params(&uri, 2, (0, 9), (0, 12), "Bar"));
    assert!(
        backend.pending_target_for(&uri).is_some(),
        "the incremental edit must still be queued for this test to exercise the race",
    );

    backend._did_change(full_replace_params(&uri, 3, "function Baz() {}\n"));

    assert!(
        backend.pending_target_for(&uri).is_none(),
        "the writer must not be left holding an edit the full replace superseded",
    );
    assert_eq!(
        published_document(&backend, &uri).source,
        "function Baz() {}\n",
        "the full replace text is what the snapshot holds",
    );
}
