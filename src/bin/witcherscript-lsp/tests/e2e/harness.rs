use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ClientSocket, MainLoop};
use lsp_types::notification::{DidSaveTextDocument, Initialized};
use lsp_types::request::{DocumentDiagnosticRequest, Initialize, Request};
use lsp_types::{
    ClientCapabilities, Diagnostic, DiagnosticClientCapabilities, DidSaveTextDocumentParams,
    DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    InitializeParams, InitializeResult, InitializedParams, PartialResultParams, ServerCapabilities,
    TextDocumentClientCapabilities, TextDocumentIdentifier, Url, WorkDoneProgressParams,
};
use serde_json::Value;
use tokio::io::{split, DuplexStream, ReadHalf, WriteHalf};
use tokio::task::JoinHandle;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;

use super::super::jsonrpc_client::JsonRpcClient;
use crate::backend::Backend;
use crate::config::Config;

enum PanicRequest {}
impl Request for PanicRequest {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "test/panic";
}

pub(crate) struct LspClient {
    rpc: JsonRpcClient<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>,
    init_result: InitializeResult,
    _server: JoinHandle<()>,
}

impl LspClient {
    pub(crate) async fn spawn() -> Self {
        Self::spawn_with(None).await
    }

    pub(crate) async fn spawn_open_files_scope() -> Self {
        Self::spawn_with(Some(serde_json::json!({
            "diagnostics": { "scope": "openFiles" }
        })))
        .await
    }

    async fn spawn_with(init_options: Option<Value>) -> Self {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = split(server_io);

        let (server, _client_socket) = MainLoop::new_server(move |client: ClientSocket| {
            let config = Arc::new(ArcSwap::from_pointee(Config::default()));
            let backend = Backend::new(client, config);

            let mut router: Router<Backend> = Router::from_language_server(backend);
            crate::register_notification_handlers(&mut router);
            router.request::<PanicRequest, _>(|_backend, _params| async move {
                panic!("intentional panic from test/panic handler");
                #[allow(unreachable_code)]
                Ok(Value::Null)
            });

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
                capabilities: ClientCapabilities {
                    text_document: Some(TextDocumentClientCapabilities {
                        diagnostic: Some(DiagnosticClientCapabilities::default()),
                        ..TextDocumentClientCapabilities::default()
                    }),
                    ..ClientCapabilities::default()
                },
                initialization_options: init_options,
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

    pub(crate) async fn close(&mut self, uri: &Url) {
        self.rpc.did_close(uri).await;
    }

    pub(crate) async fn did_save(&mut self, uri: &Url) {
        self.rpc
            .notify::<DidSaveTextDocument>(DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                text: None,
            })
            .await;
    }

    pub(crate) async fn notify<N: lsp_types::notification::Notification>(
        &mut self,
        params: N::Params,
    ) {
        self.rpc.notify::<N>(params).await;
    }

    pub(crate) async fn request<R: Request>(&mut self, params: R::Params) -> R::Result {
        self.rpc.request::<R>(params).await
    }

    pub(crate) async fn raw_request(&mut self, method: &str, params: Value) -> Value {
        self.rpc.raw_request(method, params).await
    }

    pub(crate) async fn pull_diagnostics(&mut self, uri: &Url) -> Vec<Diagnostic> {
        let report = self
            .request::<DocumentDiagnosticRequest>(DocumentDiagnosticParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                identifier: None,
                previous_result_id: None,
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await;
        match report {
            DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
                full.full_document_diagnostic_report.items
            }
            DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(_)) => {
                panic!("pull_diagnostics requested without previous_result_id must return Full")
            }
            DocumentDiagnosticReportResult::Partial(_) => {
                panic!("server returned a partial report unexpectedly")
            }
        }
    }
}
