mod backend;
mod convert;
mod cst_cache;
mod indexing;
mod logging;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8};
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};
use tower_lsp::lsp_types::MessageType;
use tower_lsp::{ClientSocket, LspService, Server};
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
use witcherscript_parser::builtins::load_builtins_index;
use witcherscript_parser::resolve::WorkspaceIndex;
use witcherscript_parser::script_env::ScriptEnvironment;

use backend::Backend;
use logging::{level_to_u8, LspLogSender, DEFAULT_LOG_LEVEL};

#[tokio::main]
async fn main() {
    let listen_port = parse_listen_port();

    let (log_tx, log_rx) = mpsc::unbounded_channel::<(MessageType, String)>();
    let log_level = Arc::new(AtomicU8::new(level_to_u8(DEFAULT_LOG_LEVEL)));

    init_tracing(log_tx, Arc::clone(&log_level), listen_port.is_some());

    let (service, socket) = build_service(log_rx, log_level);

    match listen_port {
        Some(port) => serve_tcp(port, service, socket).await,
        None => serve_stdio(service, socket).await,
    }
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
    log_level: Arc<AtomicU8>,
    tcp_mode: bool,
) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if tcp_mode {
            tracing_subscriber::EnvFilter::new(
                "warn,witcherscript_lsp=trace,witcherscript_parser=trace",
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
                .with_filter(env_filter),
        )
        .with(LspLogSender {
            sender: log_tx,
            min_level: log_level,
        })
        .init();
}

fn build_service(
    mut log_rx: mpsc::UnboundedReceiver<(MessageType, String)>,
    log_level: Arc<AtomicU8>,
) -> (LspService<Backend>, ClientSocket) {
    LspService::build(move |client| {
        let c = client.clone();
        tokio::spawn(async move {
            while let Some((kind, msg)) = log_rx.recv().await {
                c.log_message(kind, msg).await;
            }
        });
        Backend {
            client,
            log_level,
            documents: Arc::new(Mutex::new(HashMap::new())),
            published_diagnostics: Arc::new(Mutex::new(HashMap::new())),
            workspace_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            workspace_documents: Arc::new(Mutex::new(HashMap::new())),
            workspace_roots: Arc::new(Mutex::new(Vec::new())),
            files_exclude: Arc::new(Mutex::new(Vec::new())),
            base_scripts_path: Arc::new(Mutex::new(None)),
            additional_script_dirs: Arc::new(Mutex::new(Vec::new())),
            auto_load_mod_shared_imports: Arc::new(AtomicBool::new(true)),
            base_scripts_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
            base_scripts_documents: Arc::new(Mutex::new(HashMap::new())),
            builtins_index: Arc::new(load_builtins_index()),
            script_env: Arc::new(Mutex::new(ScriptEnvironment::default())),
            cst_diag_cache: Arc::new(Mutex::new(HashMap::new())),
            formatter_line_limit: Arc::new(AtomicU32::new(100)),
            formatter_compact_colon: Arc::new(AtomicBool::new(false)),
            formatter_align_member_colons: Arc::new(AtomicBool::new(false)),
            initial_index_done: Arc::new(Mutex::new(false)),
        }
    })
    .custom_method(
        "witcherscript/builtinSource",
        Backend::handle_builtin_source,
    )
    .finish()
}

async fn serve_stdio(service: LspService<Backend>, socket: ClientSocket) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    Server::new(stdin, stdout, socket).serve(service).await;
}

async fn serve_tcp(port: u16, service: LspService<Backend>, socket: ClientSocket) {
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
    Server::new(read, write, socket).serve(service).await;
}
