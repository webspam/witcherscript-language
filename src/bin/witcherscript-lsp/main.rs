mod backend;
mod config;
mod convert;
mod cst_cache;
mod diagnostics_publish;
mod indexing;
mod logging;
#[cfg(test)]
mod tests;
mod watcher;

use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::ops::ControlFlow;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ClientSocket, LanguageClient, ResponseError};
use futures::FutureExt;
use lsp_types::notification::{
    DidChangeWorkspaceFolders, DidCreateFiles, DidDeleteFiles, DidRenameFiles, DidSaveTextDocument,
    WillSaveTextDocument, WorkDoneProgressCancel,
};
use lsp_types::request::Request;
use lsp_types::{LogMessageParams, MessageType};
use serde_json::Value;
use tokio::sync::{mpsc, Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
use witcherscript_language::builtins::load_builtins_index;
use witcherscript_language::resolve::WorkspaceIndex;
use witcherscript_language::script_env::ScriptEnvironment;

use backend::{Backend, DocOp};
use config::Config;
use logging::LspLogSender;

type LogRxHolder = Arc<Mutex<Option<mpsc::UnboundedReceiver<(MessageType, String)>>>>;

enum BuiltinSourceRequest {}
impl Request for BuiltinSourceRequest {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "witcherscript/builtinSource";
}

#[tokio::main]
async fn main() {
    let listen_port = parse_listen_port();

    let (log_tx, log_rx) = mpsc::unbounded_channel::<(MessageType, String)>();
    let config = Arc::new(ArcSwap::from_pointee(Config::default()));

    init_tracing(log_tx, Arc::clone(&config), listen_port.is_some());

    let log_rx_holder = Arc::new(Mutex::new(Some(log_rx)));
    let config_for_backend = Arc::clone(&config);

    let (server, _client_socket) = async_lsp::MainLoop::new_server(move |client: ClientSocket| {
        spawn_log_forwarder(client.clone(), Arc::clone(&log_rx_holder));

        let (doc_ops_tx, mut doc_ops_rx) = mpsc::unbounded_channel::<DocOp>();

        let backend = Backend {
            client,
            config: Arc::clone(&config_for_backend),
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
                let result = std::panic::AssertUnwindSafe(async move {
                    backend.dispatch_doc_op(op).await;
                })
                .catch_unwind()
                .await;
                if let Err(panic) = result {
                    let payload = panic
                        .downcast_ref::<&'static str>()
                        .copied()
                        .map(str::to_string)
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "<non-string panic payload>".to_string());
                    tracing::error!(panic = %payload, "doc op handler panicked; continuing");
                }
            }
        });

        let mut router: Router<Backend> = Router::from_language_server(backend);
        router.request::<BuiltinSourceRequest, _>(|backend, params| {
            let backend = backend.clone();
            async move { backend.handle_builtin_source(params).await }
        });

        ignore_unhandled_notifications(&mut router);

        ServiceBuilder::new()
            .layer(TracingLayer::default())
            .layer(LifecycleLayer::default())
            .layer(CatchUnwindLayer::default())
            .layer(ConcurrencyLayer::default())
            .service(router)
    });

    match listen_port {
        Some(port) => serve_tcp(port, server).await,
        None => serve_stdio(server).await,
    }
}

/// async-lsp's default for an unhandled notification (e.g. didSave) is to terminate the server.
pub(crate) fn ignore_unhandled_notifications(router: &mut Router<Backend>) {
    router
        .notification::<DidSaveTextDocument>(|_, _| ControlFlow::Continue(()))
        .notification::<WillSaveTextDocument>(|_, _| ControlFlow::Continue(()))
        .notification::<DidCreateFiles>(|_, _| ControlFlow::Continue(()))
        .notification::<DidRenameFiles>(|_, _| ControlFlow::Continue(()))
        .notification::<DidDeleteFiles>(|_, _| ControlFlow::Continue(()))
        .notification::<DidChangeWorkspaceFolders>(|_, _| ControlFlow::Continue(()))
        .notification::<WorkDoneProgressCancel>(|_, _| ControlFlow::Continue(()))
        .unhandled_notification(|_, notif| {
            tracing::debug!(method = %notif.method, "ignoring unhandled notification");
            ControlFlow::Continue(())
        });
}

fn parse_listen_port() -> Option<u16> {
    let mut args = std::env::args().skip(1);
    let arg = args.next()?;
    match arg.as_str() {
        "--listen" => {
            let Some(raw) = args.next() else {
                eprintln!("witcherscript-lsp: --listen requires a port number");
                std::process::exit(2);
            };
            match raw.parse::<u16>() {
                Ok(p) => Some(p),
                Err(_) => {
                    eprintln!("witcherscript-lsp: invalid --listen port: {raw}");
                    std::process::exit(2);
                }
            }
        }
        "--stdio" => None,
        "--help" | "-h" => {
            eprintln!("Usage: witcherscript-lsp [--stdio | --listen <port>]");
            std::process::exit(0);
        }
        other => {
            eprintln!("witcherscript-lsp: unknown argument: {other}");
            std::process::exit(2);
        }
    }
}

fn init_tracing(
    log_tx: mpsc::UnboundedSender<(MessageType, String)>,
    config: Arc<ArcSwap<Config>>,
    tcp_mode: bool,
) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if tcp_mode {
            tracing_subscriber::EnvFilter::new(
                "warn,witcherscript_lsp=trace,witcherscript_language=trace",
            )
        } else {
            tracing_subscriber::EnvFilter::default()
        }
    });

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(std::io::stderr().is_terminal())
                .with_span_events(FmtSpan::CLOSE)
                .with_filter(env_filter),
        )
        .with(LspLogSender {
            sender: log_tx,
            config,
        })
        .init();
}

fn spawn_log_forwarder(mut client: ClientSocket, log_rx_holder: LogRxHolder) {
    tokio::spawn(async move {
        let mut log_rx = match log_rx_holder.lock().await.take() {
            Some(rx) => rx,
            None => return,
        };
        while let Some((typ, message)) = log_rx.recv().await {
            if client
                .log_message(LogMessageParams { typ, message })
                .is_err()
            {
                break;
            }
        }
    });
}

async fn serve_stdio<S>(server: async_lsp::MainLoop<S>)
where
    S: async_lsp::LspService<Response = Value> + 'static,
    S::Future: Send,
    ResponseError: From<S::Error>,
{
    let stdin = tokio::io::stdin().compat();
    let stdout = tokio::io::stdout().compat_write();
    if let Err(err) = server.run_buffered(stdin, stdout).await {
        eprintln!("witcherscript-lsp: server error: {err}");
        std::process::exit(1);
    }
}

async fn serve_tcp<S>(port: u16, server: async_lsp::MainLoop<S>)
where
    S: async_lsp::LspService<Response = Value> + 'static,
    S::Future: Send,
    ResponseError: From<S::Error>,
{
    let bind_addr = ("127.0.0.1", port);
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("witcherscript-lsp: failed to bind 127.0.0.1:{port}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("witcherscript-lsp: listening on tcp://127.0.0.1:{port} (waiting for client)");
    let (stream, peer) = match listener.accept().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("witcherscript-lsp: accept failed: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("witcherscript-lsp: client connected from {peer}");
    let (read, write) = stream.into_split();
    if let Err(err) = server
        .run_buffered(read.compat(), write.compat_write())
        .await
    {
        eprintln!("witcherscript-lsp: server error: {err}");
        std::process::exit(1);
    }
}
