use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use arc_swap::ArcSwap;
use async_lsp::ClientSocket;
use async_lsp::router::Router;
use lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};

use crate::backend::Backend;
use crate::config::{Config, DiagnosticsScope};

pub(crate) struct LocalTempDir {
    path: PathBuf,
}

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

impl LocalTempDir {
    pub(crate) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("mkdir tempdir");
        Self { path }
    }

    // Roots under target/tmp (CLAUDE.local.md) with a per-process-unique suffix so parallel tests never collide.
    pub(crate) fn under_target(name: &str) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("tmp")
            .join(format!("{name}-{}-{id}", std::process::id()));
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("mkdir tempdir");
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LocalTempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
}

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
