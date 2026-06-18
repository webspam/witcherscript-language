use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ClientSocket, ErrorCode, MainLoop};
use lsp_types::notification::{DidSaveTextDocument, Initialized};
use lsp_types::request::{DocumentDiagnosticRequest, Initialize, Request, WorkspaceSymbolRequest};
use lsp_types::{
    ClientCapabilities, CodeLensWorkspaceClientCapabilities, Diagnostic,
    DiagnosticClientCapabilities, DidSaveTextDocumentParams, DocumentDiagnosticParams,
    DocumentDiagnosticReport, DocumentDiagnosticReportResult, InitializeParams, InitializeResult,
    InitializedParams, InlayHintWorkspaceClientCapabilities, PartialResultParams,
    SemanticTokensWorkspaceClientCapabilities, ServerCapabilities, TextDocumentClientCapabilities,
    TextDocumentIdentifier, Url, WorkDoneProgressParams, WorkspaceClientCapabilities,
    WorkspaceFolder, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};
use serde_json::{Value, json};
use tokio::io::{DuplexStream, ReadHalf, WriteHalf, split};
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

const CANCELLATION_RETRY_LIMIT: usize = 200;
const PULL_RETRY_LIMIT: usize = 50;
const RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(5);

fn error_code(err: &Value) -> Option<ErrorCode> {
    err.get("code")
        .and_then(serde_json::Value::as_i64)
        .map(|c| ErrorCode(i32::try_from(c).expect("JSON-RPC error codes are i32")))
}

// A ServerCancelled the client should re-trigger: retriggerRequest defaults to true when absent.
fn is_retriggerable_cancellation(err: &Value) -> bool {
    if error_code(err) != Some(ErrorCode::SERVER_CANCELLED) {
        return false;
    }
    err.pointer("/data/retriggerRequest")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

pub(crate) struct LspClient {
    rpc: JsonRpcClient<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>,
    init_result: InitializeResult,
    _server: JoinHandle<()>,
}

enum ConfigReplies {
    Answer,
    Hold,
}

enum ViewRefresh {
    Enabled,
    Disabled,
}

pub(crate) struct LspClientBuilder {
    roots: Vec<PathBuf>,
    init_options: Option<Value>,
    config_overrides: HashMap<String, Value>,
    view_refresh: ViewRefresh,
    config_replies: ConfigReplies,
    wait_until_indexed: bool,
}

impl LspClientBuilder {
    pub(crate) fn new() -> Self {
        Self {
            roots: Vec::new(),
            init_options: None,
            config_overrides: HashMap::new(),
            view_refresh: ViewRefresh::Disabled,
            config_replies: ConfigReplies::Answer,
            wait_until_indexed: true,
        }
    }

    pub(crate) fn root(mut self, dir: &Path) -> Self {
        self.roots.push(dir.to_path_buf());
        self
    }

    pub(crate) fn init_options(mut self, options: Value) -> Self {
        self.init_options = Some(options);
        self
    }

    pub(crate) fn config_override(mut self, section: &str, value: Value) -> Self {
        self.config_overrides.insert(section.to_string(), value);
        self
    }

    // Holding the workspace/configuration reply keeps the server deterministically pre-index until wait_until_indexed().
    pub(crate) fn hold_config(mut self) -> Self {
        self.config_replies = ConfigReplies::Hold;
        self
    }

    pub(crate) fn view_refresh(mut self) -> Self {
        self.view_refresh = ViewRefresh::Enabled;
        self
    }

    pub(crate) fn no_index_wait(mut self) -> Self {
        self.wait_until_indexed = false;
        self
    }

    pub(crate) async fn spawn(self) -> LspClient {
        LspClient::spawn_from_builder(self).await
    }
}

impl LspClient {
    pub(crate) async fn spawn() -> Self {
        LspClientBuilder::new().spawn().await
    }

    pub(crate) async fn spawn_with_held_config() -> Self {
        LspClientBuilder::new()
            .hold_config()
            .no_index_wait()
            .spawn()
            .await
    }

    pub(crate) async fn spawn_open_files_scope() -> Self {
        LspClientBuilder::new()
            .init_options(json!({ "diagnostics": { "scope": "openFiles" } }))
            .spawn()
            .await
    }

    // No readiness wait: the post-index view refresh is the caller's signal, and wait_until_indexed would consume it.
    pub(crate) async fn spawn_with_view_refresh() -> Self {
        LspClientBuilder::new()
            .view_refresh()
            .no_index_wait()
            .spawn()
            .await
    }

    async fn spawn_from_builder(builder: LspClientBuilder) -> Self {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = split(server_io);

        let (server, _client_socket) = MainLoop::new_server(move |client: ClientSocket| {
            let config = Arc::new(ArcSwap::from_pointee(Config::default()));
            let backend = Backend::new(client, config);

            let mut router: Router<Backend> = Router::from_language_server(backend);
            crate::register_custom_requests(&mut router);
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
        if matches!(builder.config_replies, ConfigReplies::Hold) {
            rpc.hold_config_replies();
        }
        if !builder.config_overrides.is_empty() {
            rpc.set_config_overrides(builder.config_overrides);
        }

        let (root_uri, workspace_folders) = if builder.roots.is_empty() {
            (None, None)
        } else {
            let folders: Vec<WorkspaceFolder> = builder
                .roots
                .iter()
                .enumerate()
                .map(|(i, dir)| WorkspaceFolder {
                    uri: Url::from_directory_path(dir).expect("workspace root path -> URI"),
                    name: format!("fixture{i}"),
                })
                .collect();
            let root_uri = folders.first().map(|f| f.uri.clone());
            (root_uri, Some(folders))
        };

        // root_uri is deprecated in the LSP but real editors still send it alongside workspace_folders.
        #[allow(deprecated)]
        let init_result: <Initialize as Request>::Result = rpc
            .request::<Initialize>(InitializeParams {
                capabilities: ClientCapabilities {
                    text_document: Some(TextDocumentClientCapabilities {
                        diagnostic: Some(DiagnosticClientCapabilities::default()),
                        ..TextDocumentClientCapabilities::default()
                    }),
                    workspace: matches!(builder.view_refresh, ViewRefresh::Enabled).then_some(
                        WorkspaceClientCapabilities {
                            code_lens: Some(CodeLensWorkspaceClientCapabilities {
                                refresh_support: Some(true),
                            }),
                            semantic_tokens: Some(SemanticTokensWorkspaceClientCapabilities {
                                refresh_support: Some(true),
                            }),
                            inlay_hint: Some(InlayHintWorkspaceClientCapabilities {
                                refresh_support: Some(true),
                            }),
                            ..WorkspaceClientCapabilities::default()
                        },
                    ),
                    ..ClientCapabilities::default()
                },
                initialization_options: builder.init_options,
                root_uri,
                workspace_folders,
                ..InitializeParams::default()
            })
            .await;
        rpc.notify::<Initialized>(InitializedParams {}).await;

        let mut client = LspClient {
            rpc,
            init_result,
            _server: server_handle,
        };
        if builder.wait_until_indexed {
            client.wait_until_indexed().await;
        }
        client
    }

    pub(crate) fn server_capabilities(&self) -> &ServerCapabilities {
        &self.init_result.capabilities
    }

    // workspace/symbol parks until the initial index is ready, so tests never race the pre-index empty diagnostic answers.
    pub(crate) async fn wait_until_indexed(&mut self) {
        self.rpc.release_config_replies().await;
        let _: Option<WorkspaceSymbolResponse> = self
            .rpc
            .request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams::default())
            .await;
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

    // Re-triggers a ServerCancelled pull, as a real client does, until the report is ready.
    pub(crate) async fn request_when_ready<R: Request>(&mut self, params: R::Params) -> R::Result {
        let params = serde_json::to_value(params).expect("serialize request params");
        for _ in 0..CANCELLATION_RETRY_LIMIT {
            let v = self.raw_request(R::METHOD, params.clone()).await;
            if let Some(err) = v.get("error") {
                assert!(
                    is_retriggerable_cancellation(err),
                    "request {} returned error: {err}",
                    R::METHOD
                );
                tokio::time::sleep(RETRY_BACKOFF).await;
                continue;
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

    pub(crate) async fn wait_for_server_requests(&mut self, methods: &[&str]) -> bool {
        self.rpc.wait_for_server_requests(methods).await
    }

    pub(crate) async fn pull_diagnostics(&mut self, uri: &Url) -> Vec<Diagnostic> {
        // A real client re-pulls on CONTENT_MODIFIED (mid-edit) and on a retriggerable
        // ServerCancelled (base scripts still indexing), so the harness must too.
        let params = serde_json::to_value(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .expect("serialize diagnostic params");

        for _ in 0..PULL_RETRY_LIMIT {
            let v = self
                .raw_request(DocumentDiagnosticRequest::METHOD, params.clone())
                .await;
            if let Some(err) = v.get("error") {
                if error_code(err) == Some(ErrorCode::CONTENT_MODIFIED) {
                    continue;
                }
                if is_retriggerable_cancellation(err) {
                    tokio::time::sleep(RETRY_BACKOFF).await;
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
