mod backend;
mod convert;
mod indexing;
mod logging;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8};
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};
use tower_lsp::lsp_types::MessageType;
use tower_lsp::{LspService, Server};
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
use witcherscript_parser::resolve::WorkspaceIndex;
use witcherscript_parser::script_env::ScriptEnvironment;

use backend::Backend;
use logging::{level_to_u8, LspLogSender, DEFAULT_LOG_LEVEL};

#[tokio::main]
async fn main() {
    let (log_tx, mut log_rx) = mpsc::unbounded_channel::<(MessageType, String)>();
    let log_level = Arc::new(AtomicU8::new(level_to_u8(DEFAULT_LOG_LEVEL)));

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(tracing_subscriber::EnvFilter::from_default_env()),
        )
        .with(LspLogSender {
            sender: log_tx,
            min_level: Arc::clone(&log_level),
        })
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(move |client| {
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
            script_env: Arc::new(Mutex::new(ScriptEnvironment::default())),
            formatter_line_limit: Arc::new(AtomicU32::new(100)),
            formatter_compact_colon: Arc::new(AtomicBool::new(false)),
            formatter_align_member_colons: Arc::new(AtomicBool::new(false)),
            initial_index_done: Arc::new(Mutex::new(false)),
        }
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
