use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_lsp::router::Router;
use async_lsp::ClientSocket;
use lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};
use tokio::sync::mpsc;

use crate::backend::{Backend, DocOp};
use crate::config::{Config, DiagnosticsScope};

pub(super) struct LocalTempDir {
    path: PathBuf,
}

impl LocalTempDir {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("mkdir tempdir");
        Self { path }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LocalTempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
}

pub(super) fn make_backend() -> Backend {
    let (_main_loop, client) =
        async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
    let (doc_ops_tx, _doc_ops_rx) = mpsc::unbounded_channel();
    let config = Arc::new(ArcSwap::from_pointee(Config {
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));
    Backend::new(client, config, doc_ops_tx)
}

pub(super) fn write_script(dir: &Path, rel: &str, contents: &str) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(&path, contents).expect("write script");
    path
}

pub(super) fn make_game_dir(temp: &Path, rel: &str, contents: &str) -> (PathBuf, Url) {
    let game_dir = temp.join("game");
    let full_rel = Path::new("content")
        .join("content0")
        .join("scripts")
        .join(rel);
    let path = write_script(&game_dir, full_rel.to_str().unwrap(), contents);
    let url = Url::from_file_path(&path).expect("base path -> url");
    (game_dir, url)
}

pub(super) async fn indexed_legacy_override(name: &str) -> (LocalTempDir, Backend, Url, Url) {
    let temp = LocalTempDir::new(name);
    let (game_dir, _base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let override_path = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );
    let new_path = write_script(&legacy_dir, "game/MyNewMod.ws", "class CMyNewMod {}\n");
    let override_url = Url::from_file_path(&override_path).expect("override path -> url");
    let new_url = Url::from_file_path(&new_path).expect("new path -> url");

    let backend = make_backend();
    *backend.base_scripts_path.lock().await = Some(game_dir);
    *backend.legacy_script_dirs.lock().await = vec![legacy_dir];
    backend.index_base_scripts().await;
    (temp, backend, override_url, new_url)
}

pub(super) fn open_op(uri: &Url, text: &str) -> DocOp {
    DocOp::Open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "witcherscript".to_string(),
            version: 1,
            text: text.to_string(),
        },
    })
}
