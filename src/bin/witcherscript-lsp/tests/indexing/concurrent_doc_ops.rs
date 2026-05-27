use std::sync::atomic::Ordering;
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::router::Router;
use async_lsp::ClientSocket;
use lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, Position, Range,
    TextDocumentContentChangeEvent, TextDocumentItem, Url, VersionedTextDocumentIdentifier,
};
use witcherscript_language::semantic_tokens::collect_semantic_tokens;

use crate::backend::Backend;
use crate::config::{Config, DiagnosticsScope};

fn make_backend() -> Backend {
    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));
    Backend::new(client, config)
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

#[test]
fn did_change_applies_incremental_edits() {
    let backend = make_backend();

    let uri: Url = "file:///rapid_changes.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "abc"));
    backend._did_change(change_params(&uri, 2, (0, 3), (0, 3), "def"));
    backend._did_change(change_params(&uri, 3, (0, 5), (0, 6), ""));

    let docs = backend.documents.lock();
    assert_eq!(
        docs.get(&uri).map(|d| d.source.as_str()),
        Some("abcde"),
        "each did_change must compose with prior buffer state and updated line index",
    );
}

#[test]
fn did_change_tracks_each_document_independently() {
    let backend = make_backend();

    let uri_a: Url = "file:///a.ws".parse().unwrap();
    let uri_b: Url = "file:///b.ws".parse().unwrap();
    backend._did_open(open_params(&uri_a, "a"));
    backend._did_open(open_params(&uri_b, "b"));

    backend._did_change(change_params(&uri_a, 2, (0, 1), (0, 1), "X"));
    backend._did_change(change_params(&uri_b, 2, (0, 1), (0, 1), "Y"));
    backend._did_change(change_params(&uri_a, 3, (0, 2), (0, 2), "X"));
    backend._did_change(change_params(&uri_b, 3, (0, 2), (0, 2), "Y"));

    let docs = backend.documents.lock();
    assert_eq!(
        docs.get(&uri_a).map(|d| d.source.as_str()),
        Some("aXX"),
        "edits to one document must not leak into another's buffer",
    );
    assert_eq!(
        docs.get(&uri_b).map(|d| d.source.as_str()),
        Some("bYY"),
        "edits to one document must not leak into another's buffer",
    );
}

// Regression test for #94: when did_change runs synchronously in the notification
// handler, the next handler that locks `documents` must observe the post-change
// source. The old mpsc dispatcher deferred did_change to a worker task, letting a
// concurrent semanticTokens handler read stale `documents` before the worker ran.
#[test]
fn semantic_tokens_after_did_change_sees_new_source() {
    let backend = make_backend();
    let uri: Url = "file:///regression94.ws".parse().unwrap();

    backend._did_open(open_params(&uri, "class C {}\n"));
    backend._did_change(change_params(
        &uri,
        2,
        (0, 0),
        (1, 0),
        "class CRenamed {}\n",
    ));

    let documents = backend.documents.lock();
    let document = documents.get(&uri).expect("document present after change");
    assert_eq!(
        document.source, "class CRenamed {}\n",
        "did_change must have applied before any read can observe `documents`",
    );

    let handles = backend.db_handles_for(&uri);
    let db = handles.db();
    let tokens = collect_semantic_tokens(uri.as_str(), document, &db);
    assert!(
        !tokens.is_empty(),
        "semantic tokens must be produced from the post-change source",
    );
}

// Demonstrates the version-counter discard pattern: when diagnostic_version is bumped past
// the version a publish was issued for, that publish bails before recording anything,
// so a stale spawned task can't overwrite a newer one's result.
#[test]
fn publish_open_diagnostics_bails_when_version_advanced() {
    let backend = {
        let (_main_loop, client) =
            async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
        let config = Arc::new(ArcSwap::from_pointee(Config {
            diagnostics_scope: DiagnosticsScope::Workspace,
            ..Config::default()
        }));
        let backend = Backend::new(client, config);
        backend.initial_index_done.store(true, Ordering::Release);
        backend
    };
    let uri: Url = "file:///stale.ws".parse().unwrap();
    backend._did_open(open_params(&uri, "class CBroken {\n"));

    let stale_version = backend.diagnostic_version.load(Ordering::Acquire);
    backend
        .published_diagnostics
        .lock()
        .insert(uri.clone(), Vec::new());
    backend
        .diagnostic_version
        .store(stale_version + 100, Ordering::Release);

    backend.publish_open_diagnostics(stale_version);

    assert_eq!(
        backend
            .published_diagnostics
            .lock()
            .get(&uri)
            .map(|d| d.len()),
        Some(0),
        "a stale-version publish must not overwrite the already-recorded diagnostics",
    );
}
