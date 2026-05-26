use std::sync::Arc;

use lsp_types::Url;

use crate::config::{Config, DiagnosticsScope};

use super::legacy_helpers::{make_backend, make_game_dir, write_script, LocalTempDir};

#[tokio::test]
async fn manifest_scripts_root_suppresses_a_base_script() {
    let temp = LocalTempDir::new("ws_manifest_routing_override");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");

    let project_dir = temp.path().join("Mods/modFriendlyFocus/content");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("witcherscript.toml"),
        "[content]\nname = \"modFriendlyFocus\"\nversion = \"1.0.0\"\nscripts_root = \"scripts\"\n",
    )
    .unwrap();
    let override_path = write_script(
        &project_dir.join("scripts"),
        "game/r4Player.ws",
        "class CR4Player {}\n// from manifest\n",
    );
    let override_url = Url::from_file_path(&override_path).expect("override -> url");

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    *backend.base_scripts_path.lock() = Some(game_dir);

    backend.refresh_manifest_legacy_dirs();
    backend.index_base_scripts().await;

    assert!(
        backend
            .base_scripts_documents
            .lock()
            .contains_key(base_url.as_str()),
        "the base script must stay in the base index for references"
    );
    assert!(
        backend
            .suppressed_base_uris
            .lock()
            .contains(base_url.as_str()),
        "manifest scripts_root override must suppress the base URI"
    );

    let ws_docs = backend.workspace_documents.lock();
    assert!(
        ws_docs.contains_key(override_url.as_str()),
        "manifest override should land in workspace_documents; keys: {:?}",
        ws_docs.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn flag_off_skips_discovery_and_leaves_base_unsuppressed() {
    let temp = LocalTempDir::new("ws_manifest_routing_flag_off");
    let (game_dir, base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");

    let project_dir = temp.path().join("Mods/modX/content");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("witcherscript.toml"),
        "[content]\nscripts_root = \"scripts\"\n",
    )
    .unwrap();
    write_script(
        &project_dir.join("scripts"),
        "game/r4Player.ws",
        "class CR4Player {}\n// override\n",
    );

    let backend = make_backend();
    backend.config.store(Arc::new(Config {
        auto_detect_project_manifests: false,
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));
    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    *backend.base_scripts_path.lock() = Some(game_dir);

    backend.refresh_manifest_legacy_dirs();
    backend.index_base_scripts().await;

    assert!(
        backend.manifest_legacy_dirs.lock().is_empty(),
        "discovery must not run when auto_detect_project_manifests is false"
    );
    assert!(
        backend
            .base_scripts_documents
            .lock()
            .contains_key(base_url.as_str()),
        "base script must remain present when discovery is disabled"
    );
    assert!(
        !backend
            .suppressed_base_uris
            .lock()
            .contains(base_url.as_str()),
        "base script must not be suppressed when discovery is disabled"
    );
}
