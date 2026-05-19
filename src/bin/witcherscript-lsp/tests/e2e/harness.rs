use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ClientSocket, MainLoop};
use futures::FutureExt;
use lsp_types::notification::{
    DidChangeTextDocument, DidOpenTextDocument, Initialized, Notification, PublishDiagnostics,
};
use lsp_types::request::{Initialize, Request};
use lsp_types::{
    ClientCapabilities, Diagnostic, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    InitializeParams, InitializedParams, PublishDiagnosticsParams, TextDocumentContentChangeEvent,
    TextDocumentItem, Url, VersionedTextDocumentIdentifier,
};
use serde_json::{json, Value};
use tokio::io::{
    AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf,
};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;
use witcherscript_language::builtins::load_builtins_index;
use witcherscript_language::resolve::WorkspaceIndex;
use witcherscript_language::script_env::ScriptEnvironment;

use crate::backend::{Backend, DocOp};
use crate::config::Config;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct LspClient {
    write: WriteHalf<DuplexStream>,
    read: BufReader<ReadHalf<DuplexStream>>,
    next_id: i64,
    diagnostics: HashMap<Url, Vec<Diagnostic>>,
    _server: JoinHandle<()>,
}

impl LspClient {
    pub(crate) async fn spawn() -> Self {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server_io);

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

        let (read, write) = tokio::io::split(client_io);
        let mut client = LspClient {
            write,
            read: BufReader::new(read),
            next_id: 1,
            diagnostics: HashMap::new(),
            _server: server_handle,
        };

        let _: <Initialize as Request>::Result = client
            .request::<Initialize>(InitializeParams {
                capabilities: ClientCapabilities::default(),
                ..InitializeParams::default()
            })
            .await;
        client.notify::<Initialized>(InitializedParams {}).await;

        client
    }

    pub(crate) async fn open(&mut self, uri: &Url, text: &str) {
        self.notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "witcherscript".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
        // did_open dispatches into a separate consumer task; wait for the
        // publishDiagnostics that follows update_open_document to confirm
        // the doc is queryable.
        let _ = self.wait_diagnostics(uri).await;
    }

    pub(crate) async fn change_full(&mut self, uri: &Url, version: i32, text: &str) {
        self.notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_string(),
            }],
        })
        .await;
        self.diagnostics.remove(uri);
        let _ = self.wait_diagnostics(uri).await;
    }

    pub(crate) async fn request<R: Request>(&mut self, params: R::Params) -> R::Result {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": R::METHOD,
            "params": params,
        });
        self.send_raw(&msg).await;

        let result = timeout(REQUEST_TIMEOUT, async {
            loop {
                let v = self.read_raw().await;
                if v.get("id").and_then(|i| i.as_i64()) == Some(id) {
                    if let Some(err) = v.get("error") {
                        panic!("request {} returned error: {err}", R::METHOD);
                    }
                    let result = v.get("result").cloned().unwrap_or(Value::Null);
                    return serde_json::from_value::<R::Result>(result.clone()).unwrap_or_else(
                        |e| panic!("decode failed for {}: {e}\nresponse: {v}", R::METHOD),
                    );
                }
                self.handle_inbound(v);
            }
        })
        .await;
        result.unwrap_or_else(|_| panic!("request {} timed out", R::METHOD))
    }

    pub(crate) async fn notify<N: Notification>(&mut self, params: N::Params) {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": N::METHOD,
            "params": params,
        });
        self.send_raw(&msg).await;
    }

    pub(crate) async fn wait_diagnostics(&mut self, uri: &Url) -> Vec<Diagnostic> {
        let result = timeout(REQUEST_TIMEOUT, async {
            loop {
                if let Some(diags) = self.diagnostics.get(uri) {
                    return diags.clone();
                }
                let v = self.read_raw().await;
                self.handle_inbound(v);
            }
        })
        .await;
        result.unwrap_or_else(|_| panic!("timed out waiting for diagnostics for {uri}"))
    }

    async fn send_raw(&mut self, msg: &Value) {
        let body = serde_json::to_vec(msg).expect("serialize message");
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.write
            .write_all(header.as_bytes())
            .await
            .expect("write header");
        self.write.write_all(&body).await.expect("write body");
        self.write.flush().await.expect("flush");
    }

    async fn read_raw(&mut self) -> Value {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let n = self.read.read_line(&mut line).await.expect("read header");
            if n == 0 {
                panic!("server closed connection");
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some(v) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(v.trim().parse().expect("Content-Length is a number"));
            }
        }
        let n = content_length.expect("missing Content-Length header");
        let mut buf = vec![0u8; n];
        self.read.read_exact(&mut buf).await.expect("read body");
        serde_json::from_slice(&buf).expect("parse JSON")
    }

    fn handle_inbound(&mut self, v: Value) {
        let Some(method) = v.get("method").and_then(|m| m.as_str()) else {
            return;
        };
        if method == PublishDiagnostics::METHOD {
            if let Some(p) = v.get("params") {
                if let Ok(params) = serde_json::from_value::<PublishDiagnosticsParams>(p.clone()) {
                    self.diagnostics.insert(params.uri, params.diagnostics);
                }
            }
        }
    }
}
