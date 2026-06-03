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
    ClientCapabilities, CodeLensWorkspaceClientCapabilities, Diagnostic,
    DiagnosticClientCapabilities, DidSaveTextDocumentParams, DocumentDiagnosticParams,
    DocumentDiagnosticReport, DocumentDiagnosticReportResult, InitializeParams, InitializeResult,
    InitializedParams, PartialResultParams, ServerCapabilities, TextDocumentClientCapabilities,
    TextDocumentIdentifier, Url, WorkDoneProgressParams, WorkspaceClientCapabilities,
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
        Self::spawn_with(None, false).await
    }

    pub(crate) async fn spawn_open_files_scope() -> Self {
        Self::spawn_with(
            Some(serde_json::json!({
                "diagnostics": { "scope": "openFiles" }
            })),
            false,
        )
        .await
    }

    pub(crate) async fn spawn_with_code_lens_refresh() -> Self {
        Self::spawn_with(None, true).await
    }

    async fn spawn_with(init_options: Option<Value>, code_lens_refresh: bool) -> Self {
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
                    workspace: code_lens_refresh.then(|| WorkspaceClientCapabilities {
                        code_lens: Some(CodeLensWorkspaceClientCapabilities {
                            refresh_support: Some(true),
                        }),
                        ..WorkspaceClientCapabilities::default()
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

    // Retries on ServerCancelled/retriggerRequest, returned while base scripts are still indexing.
    pub(crate) async fn request_when_ready<R: Request>(&mut self, params: R::Params) -> R::Result {
        const SERVER_CANCELLED: i64 = -32802;
        let params = serde_json::to_value(params).expect("serialize request params");
        for _ in 0..200 {
            let v = self.raw_request(R::METHOD, params.clone()).await;
            if let Some(err) = v.get("error") {
                if err.get("code").and_then(|c| c.as_i64()) == Some(SERVER_CANCELLED) {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    continue;
                }
                panic!("request {} returned error: {err}", R::METHOD);
            }
            let result = v.get("result").cloned().unwrap_or(Value::Null);
            return serde_json::from_value(result)
                .unwrap_or_else(|e| panic!("decode failed for {}: {e}", R::METHOD));
        }
        panic!("request {} kept returning ServerCancelled", R::METHOD);
    }

    pub(crate) async fn raw_request(&mut self, method: &str, params: Value) -> Value {
        self.rpc.raw_request(method, params).await
    }

    pub(crate) async fn wait_for_server_request(&mut self, method: &str) -> bool {
        self.rpc.wait_for_server_request(method).await
    }

    pub(crate) async fn pull_diagnostics(&mut self, uri: &Url) -> Vec<Diagnostic> {
        // A real client re-pulls on both, so the harness must too: CONTENT_MODIFIED (mid-edit)
        // and SERVER_CANCELLED/retriggerRequest (base scripts still indexing).
        const CONTENT_MODIFIED: i64 = -32801;
        const SERVER_CANCELLED: i64 = -32802;
        let params = serde_json::to_value(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .expect("serialize diagnostic params");

        for _ in 0..50 {
            let v = self
                .raw_request(DocumentDiagnosticRequest::METHOD, params.clone())
                .await;
            if let Some(err) = v.get("error") {
                let code = err.get("code").and_then(|c| c.as_i64());
                if code == Some(CONTENT_MODIFIED) {
                    continue;
                }
                if code == Some(SERVER_CANCELLED) {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    continue;
                }
                panic!("pull_diagnostics returned error: {err}");
            }
            let report: DocumentDiagnosticReportResult =
                serde_json::from_value(v.get("result").cloned().unwrap_or(Value::Null))
                    .expect("decode diagnostic report");
            return match report {
                DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
                    full.full_document_diagnostic_report.items
                }
                DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(_)) => {
                    panic!("pull_diagnostics requested without previous_result_id must return Full")
                }
                DocumentDiagnosticReportResult::Partial(_) => {
                    panic!("server returned a partial report unexpectedly")
                }
            };
        }
        panic!("pull_diagnostics kept returning a retryable error");
    }
}
