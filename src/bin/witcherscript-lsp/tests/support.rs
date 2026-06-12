use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::ClientSocket;
use async_lsp::router::Router;
use lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};

use crate::backend::Backend;
use crate::config::{Config, DiagnosticsScope};

pub(crate) fn make_backend() -> Backend {
    make_backend_with(DiagnosticsScope::None)
}

pub(crate) fn make_backend_with(scope: DiagnosticsScope) -> Backend {
    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: scope,
        ..Config::default()
    }));
    Backend::new(client, config)
}

pub(crate) fn open_params(uri: &Url, text: &str) -> DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "witcherscript".to_string(),
            version: 1,
            text: text.to_string(),
        },
    }
}
