use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::router::Router;
use async_lsp::ClientSocket;
use lsp_types::{
    DidOpenTextDocumentParams, InlayHintParams, Position, Range, TextDocumentIdentifier,
    TextDocumentItem, Url, WorkDoneProgressParams,
};

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

fn inlay_hint_params(uri: &Url) -> InlayHintParams {
    InlayHintParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: u32::MAX,
                character: 0,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    }
}

#[test]
fn inlay_hints_setting_toggles_hints() {
    let backend = make_backend();
    let uri: Url = "file:///main.ws".parse().unwrap();
    backend._did_open(open_params(
        &uri,
        "function Foo(target : int) {}\nfunction Bar() { Foo(1); }\n",
    ));

    let enabled = backend
        ._inlay_hint(inlay_hint_params(&uri))
        .expect("handler ok")
        .expect("hints present when enabled");
    assert_eq!(enabled.len(), 1, "default-on config yields the hint");

    backend.update_config(|c| c.inlay_hints = false);
    let disabled = backend
        ._inlay_hint(inlay_hint_params(&uri))
        .expect("handler ok");
    assert!(
        disabled.is_none(),
        "disabling the setting suppresses all hints"
    );
}
