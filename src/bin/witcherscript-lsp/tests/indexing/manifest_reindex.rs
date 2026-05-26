use std::sync::Arc;

use lsp_types::{FileChangeType, FileEvent, Url};

use crate::config::{Config, DiagnosticsScope};

use super::legacy_helpers::{make_backend, LocalTempDir};

fn write_manifest(temp: &std::path::Path, rel_dir: &str, scripts_subdir: &str) -> Url {
    let dir = temp.join(rel_dir);
    let scripts = dir.join(scripts_subdir);
    std::fs::create_dir_all(&scripts).expect("mkdir scripts");
    let toml_path = dir.join("witcherscript.toml");
    std::fs::write(
        &toml_path,
        format!("[content]\nscripts_root = \"{scripts_subdir}\"\n"),
    )
    .expect("write manifest");
    Url::from_file_path(&toml_path).expect("manifest path -> url")
}

#[test]
fn adding_a_manifest_via_watched_event_changes_the_dir_set() {
    let temp = LocalTempDir::new("ws_manifest_reindex_add");
    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    backend.refresh_manifest_legacy_dirs();
    assert!(backend.manifest_legacy_dirs.lock().is_empty());

    let manifest_url = write_manifest(temp.path(), "Mods/modA/content", "scripts");

    backend.apply_watched_file_events(vec![FileEvent {
        uri: manifest_url,
        typ: FileChangeType::CREATED,
    }]);

    assert_eq!(
        backend.manifest_legacy_dirs.lock().len(),
        1,
        "a CREATED manifest event must populate manifest_legacy_dirs"
    );
}

#[test]
fn manifest_event_with_no_set_change_is_a_noop() {
    let temp = LocalTempDir::new("ws_manifest_reindex_noop");
    let manifest_url = write_manifest(temp.path(), "Mods/modA", "scripts");

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    backend.refresh_manifest_legacy_dirs();
    let prev_dirs = backend.manifest_legacy_dirs.lock().clone();
    assert_eq!(prev_dirs.len(), 1, "sanity: discovery seeded one dir");

    backend.apply_watched_file_events(vec![FileEvent {
        uri: manifest_url,
        typ: FileChangeType::CHANGED,
    }]);

    assert_eq!(
        *backend.manifest_legacy_dirs.lock(),
        prev_dirs,
        "an unchanged manifest must not change the cached dir set"
    );
}

#[test]
fn deleting_a_manifest_via_watched_event_removes_the_dir() {
    let temp = LocalTempDir::new("ws_manifest_reindex_delete");
    let dir = temp.path().join("Mods/modA/content");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let toml_path = dir.join("witcherscript.toml");
    std::fs::write(&toml_path, "[content]\nscripts_root = \"scripts\"\n").unwrap();
    let manifest_url = Url::from_file_path(&toml_path).unwrap();

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    backend.refresh_manifest_legacy_dirs();
    assert_eq!(backend.manifest_legacy_dirs.lock().len(), 1);

    std::fs::remove_file(&toml_path).unwrap();
    backend.apply_watched_file_events(vec![FileEvent {
        uri: manifest_url,
        typ: FileChangeType::DELETED,
    }]);

    assert!(
        backend.manifest_legacy_dirs.lock().is_empty(),
        "deleting the manifest must drop its scripts_root from the cache"
    );
}

#[test]
fn toggling_flag_off_clears_the_cache() {
    let temp = LocalTempDir::new("ws_manifest_reindex_flag_off");
    write_manifest(temp.path(), "Mods/modA", "scripts");

    let backend = make_backend();
    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    backend.refresh_manifest_legacy_dirs();
    assert_eq!(backend.manifest_legacy_dirs.lock().len(), 1);

    backend.config.store(Arc::new(Config {
        auto_detect_project_manifests: false,
        diagnostics_scope: DiagnosticsScope::None,
        ..Config::default()
    }));
    backend.refresh_manifest_legacy_dirs();

    assert!(
        backend.manifest_legacy_dirs.lock().is_empty(),
        "flipping auto_detect off must clear the cache"
    );
}
