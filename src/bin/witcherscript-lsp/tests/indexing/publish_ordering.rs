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
fn full_replace_is_queued_after_the_incremental_edit_it_supersedes() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);
    let uri: Url = "file:///full_replace.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "function Foo() {}\n"));
    backend._did_change(change_params(&uri, 2, (0, 9), (0, 12), "Bar"));
    let incremental_target = backend
        .pending_target_for(&uri)
        .expect("the incremental edit is queued");

    backend._did_change(full_replace_params(&uri, 3, "function Baz() {}\n"));

    let queued = backend
        .clone_pending_for(&uri)
        .expect("the full replace goes through the queue");
    assert_eq!(
        queued.source, "function Baz() {}\n",
        "the queue holds the full replace text, replacing the incremental edit",
    );
    assert!(
        queued.tree.is_none(),
        "a full replace carries no prior tree, so the writer parses it from scratch",
    );
    assert!(
        queued.target_parse_version > incremental_target,
        "the full replace must publish after the incremental edit it supersedes",
    );
}

#[test]
fn incremental_edit_after_a_queued_full_replace_applies_to_the_replaced_text() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);
    let uri: Url = "file:///chained.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "function Foo() {}\n"));
    backend._did_change(full_replace_params(&uri, 2, "function Baz() {}\n"));

    backend._did_change(change_params(&uri, 3, (0, 9), (0, 12), "Qux"));

    let queued = backend
        .clone_pending_for(&uri)
        .expect("the chained edit is queued");
    assert_eq!(
        queued.source, "function Qux() {}\n",
        "the next incremental edit must chain onto the queued full replace, not the published text",
    );
}

#[test]
fn full_replace_without_the_writer_publishes_immediately() {
    let backend = make_backend();
    let uri: Url = "file:///sync_full_replace.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "function Foo() {}\n"));

    backend._did_change(full_replace_params(&uri, 2, "function Baz() {}\n"));

    assert_eq!(
        published_document(&backend, &uri).source,
        "function Baz() {}\n",
        "the synchronous path parses a tree-less edit from scratch and publishes it",
    );
}
