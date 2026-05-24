use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use async_lsp::router::Router;
use async_lsp::{ClientSocket, LanguageServer};
use lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, Position, Range,
    TextDocumentContentChangeEvent, TextDocumentItem, Url, VersionedTextDocumentIdentifier,
};
use tokio::sync::mpsc;

use crate::backend::{Backend, DocOp};
use crate::config::{Config, DiagnosticsScope};

fn make_backend() -> (Backend, mpsc::UnboundedReceiver<DocOp>) {
    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let (doc_ops_tx, doc_ops_rx) = mpsc::unbounded_channel();
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));
    let backend = Backend::new(client, config, doc_ops_tx);
    (backend, doc_ops_rx)
}

fn open_params(uri: &Url, text: &str) -> DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "witcherscript".to_string(),
            version: 1,
            text: text.to_string(),
        },
    }
}

fn change_params(
    uri: &Url,
    version: i32,
    start: (u32, u32),
    end: (u32, u32),
    text: &str,
) -> DidChangeTextDocumentParams {
    DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: start.0,
                    character: start.1,
                },
                end: Position {
                    line: end.0,
                    character: end.1,
                },
            }),
            range_length: None,
            text: text.to_string(),
        }],
    }
}

async fn wait_for(backend: &Backend, uri: &Url, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let docs = backend.documents.lock().await;
            if docs.get(uri).map(|d| d.source.as_str()) == Some(expected) {
                return;
            }
        }
        if Instant::now() > deadline {
            let docs = backend.documents.lock().await;
            panic!(
                "consumer did not produce expected source within 5s; got {:?}",
                docs.get(uri).map(|d| d.source.clone()),
            );
        }
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn rapid_did_change_submissions_apply_in_order() {
    let (mut backend, mut doc_ops_rx) = make_backend();
    let consumer_backend = backend.clone();
    tokio::spawn(async move {
        while let Some(op) = doc_ops_rx.recv().await {
            consumer_backend.dispatch_doc_op(op).await;
        }
    });

    let uri: Url = "file:///rapid_changes.ws".parse().unwrap();
    let _ = backend.did_open(open_params(&uri, "abc"));

    let _ = backend.did_change(change_params(&uri, 2, (0, 3), (0, 3), "def"));
    let _ = backend.did_change(change_params(&uri, 3, (0, 5), (0, 6), ""));

    wait_for(&backend, &uri, "abcde").await;
}

#[tokio::test]
async fn interleaved_changes_across_two_documents_apply_in_order() {
    let (mut backend, mut doc_ops_rx) = make_backend();
    let consumer_backend = backend.clone();
    tokio::spawn(async move {
        while let Some(op) = doc_ops_rx.recv().await {
            consumer_backend.dispatch_doc_op(op).await;
        }
    });

    let uri_a: Url = "file:///a.ws".parse().unwrap();
    let uri_b: Url = "file:///b.ws".parse().unwrap();
    let _ = backend.did_open(open_params(&uri_a, "a"));
    let _ = backend.did_open(open_params(&uri_b, "b"));

    let _ = backend.did_change(change_params(&uri_a, 2, (0, 1), (0, 1), "X"));
    let _ = backend.did_change(change_params(&uri_b, 2, (0, 1), (0, 1), "Y"));
    let _ = backend.did_change(change_params(&uri_a, 3, (0, 2), (0, 2), "X"));
    let _ = backend.did_change(change_params(&uri_b, 3, (0, 2), (0, 2), "Y"));

    wait_for(&backend, &uri_a, "aXX").await;
    wait_for(&backend, &uri_b, "bYY").await;
}
