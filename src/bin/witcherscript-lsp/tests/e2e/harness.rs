use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ClientSocket, MainLoop};
use futures::FutureExt;
use lsp_types::notification::Initialized;
use lsp_types::request::{Initialize, Request};
use lsp_types::{
    ClientCapabilities, Diagnostic, InitializeParams, InitializeResult, InitializedParams,
    ServerCapabilities, Url,
};
use tokio::io::{split, DuplexStream, ReadHalf, WriteHalf};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;
use witcherscript_language::builtins::load_builtins_index;
use witcherscript_language::resolve::WorkspaceIndex;
use witcherscript_language::script_env::ScriptEnvironment;

use super::super::jsonrpc_client::JsonRpcClient;
use crate::backend::{Backend, DocOp};
use crate::config::Config;

pub(crate) struct LspClient {
    rpc: JsonRpcClient<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>,
    init_result: InitializeResult,
    _server: JoinHandle<()>,
}

impl LspClient {
    pub(crate) async fn spawn() -> Self {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = split(server_io);

        let (server, _client_socket) = MainLoop::new_server(move |client: ClientSocket| {
            let (doc_ops_tx, mut doc_ops_rx) = mpsc::unbounded_channel::<DocOp>();
            let backend = Backend {
                client,
                config: Arc::new(ArcSwap::from_pointee(Config {
                    diagnostics_enabled: true,
                    ..Config::default()
                })),
                documents: Arc::new(Mutex::new(HashMap::new())),
                published_diagnostics: Arc::new(Mutex::new(HashMap::new())),
                workspace_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
                workspace_documents: Arc::new(Mutex::new(HashMap::new())),
                workspace_roots: Arc::new(Mutex::new(Vec::new())),
                files_exclude: Arc::new(Mutex::new(Vec::new())),
                base_scripts_path: Arc::new(Mutex::new(None)),
                additional_script_dirs: Arc::new(Mutex::new(Vec::new())),
                legacy_script_dirs: Arc::new(Mutex::new(Vec::new())),
                legacy_indexed_uris: Arc::new(Mutex::new(HashSet::new())),
                base_scripts_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
                base_scripts_documents: Arc::new(Mutex::new(HashMap::new())),
                builtins_index: Arc::new(load_builtins_index()),
                script_env: Arc::new(Mutex::new(ScriptEnvironment::default())),
                cst_diag_cache: Arc::new(Mutex::new(HashMap::new())),
                initial_index_done: Arc::new(AtomicBool::new(false)),
                doc_ops_tx,
            };

            let consumer_backend = backend.clone();
            tokio::spawn(async move {
                while let Some(op) = doc_ops_rx.recv().await {
                    let backend = consumer_backend.clone();
                    let _ = std::panic::AssertUnwindSafe(async move {
                        backend.dispatch_doc_op(op).await;
                    })
                    .catch_unwind()
                    .await;
                }
            });

            let router: Router<Backend> = Router::from_language_server(backend);

            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(LifecycleLayer::default())
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(router)
        });

        let server_handle = tokio::spawn(async move {
            let _ = server
                .run_buffered(server_read.compat(), server_write.compat_write())
                .await;
        });

        let (read, write) = split(client_io);
        let mut rpc = JsonRpcClient::new(read, write);

        let init_result: <Initialize as Request>::Result = rpc
            .request::<Initialize>(InitializeParams {
                capabilities: ClientCapabilities::default(),
                ..InitializeParams::default()
            })
            .await;
        rpc.notify::<Initialized>(InitializedParams {}).await;

        LspClient {
            rpc,
            init_result,
            _server: server_handle,
        }
    }

    pub(crate) fn server_capabilities(&self) -> &ServerCapabilities {
        &self.init_result.capabilities
    }

    pub(crate) async fn open(&mut self, uri: &Url, text: &str) {
        self.rpc.did_open(uri, text).await;
    }

    pub(crate) async fn change_full(&mut self, uri: &Url, version: i32, text: &str) {
        self.rpc.did_change_full(uri, version, text).await;
    }

    pub(crate) async fn request<R: Request>(&mut self, params: R::Params) -> R::Result {
        self.rpc.request::<R>(params).await
    }

    pub(crate) async fn wait_diagnostics(&mut self, uri: &Url) -> Vec<Diagnostic> {
        self.rpc.wait_diagnostics(uri).await
    }
}
