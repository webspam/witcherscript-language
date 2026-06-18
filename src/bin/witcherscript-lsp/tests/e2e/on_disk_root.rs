use std::fs;

use lsp_types::request::WorkspaceSymbolRequest;
use lsp_types::{WorkspaceSymbolParams, WorkspaceSymbolResponse};
use serde_json::json;

use super::harness::LspClientBuilder;
use crate::tests::support::LocalTempDir;

fn symbol_names(response: Option<WorkspaceSymbolResponse>) -> Vec<String> {
    match response {
        Some(WorkspaceSymbolResponse::Nested(syms)) => syms.into_iter().map(|s| s.name).collect(),
        Some(WorkspaceSymbolResponse::Flat(syms)) => syms.into_iter().map(|s| s.name).collect(),
        None => Vec::new(),
    }
}

#[tokio::test]
async fn indexes_on_disk_workspace_and_base_scripts() {
    let tmp = LocalTempDir::under_target("on-disk-root");
    let ws_root = tmp.path().join("ws");
    let base_scripts = tmp
        .path()
        .join("game")
        .join("content")
        .join("content0")
        .join("scripts");
    fs::create_dir_all(ws_root.join("scripts")).expect("mkdir ws");
    fs::create_dir_all(&base_scripts).expect("mkdir base");
    fs::write(
        ws_root.join("scripts").join("lib.ws"),
        "function UniqueWorkspaceFn() {}\n",
    )
    .expect("write ws file");
    fs::write(
        base_scripts.join("engine.ws"),
        "function UniqueBaseEngineFn() {}\n",
    )
    .expect("write base file");

    let game_dir = tmp.path().join("game");
    let mut client = LspClientBuilder::new()
        .root(&ws_root)
        .config_override(
            "witcherscript.gameDirectory",
            json!(game_dir.to_str().expect("game dir is utf-8")),
        )
        .spawn()
        .await;

    let workspace_hits = symbol_names(
        client
            .request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
                query: "UniqueWorkspaceFn".to_string(),
                ..Default::default()
            })
            .await,
    );
    assert!(
        workspace_hits.iter().any(|n| n == "UniqueWorkspaceFn"),
        "on-disk workspace scan must index the user script, got {workspace_hits:?}"
    );

    let base_hits = symbol_names(
        client
            .request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
                query: "UniqueBaseEngineFn".to_string(),
                ..Default::default()
            })
            .await,
    );
    assert!(
        base_hits.iter().any(|n| n == "UniqueBaseEngineFn"),
        "gameDirectory delivered via workspace/configuration must index base scripts, got {base_hits:?}"
    );
}
