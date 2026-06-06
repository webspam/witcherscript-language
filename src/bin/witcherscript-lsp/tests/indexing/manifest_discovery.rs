use super::legacy_helpers::{make_backend, LocalTempDir};

fn write_manifest(temp: &std::path::Path, rel_dir: &str, scripts_subdir: &str) {
    let dir = temp.join(rel_dir);
    let scripts = dir.join(scripts_subdir);
    std::fs::create_dir_all(&scripts).expect("mkdir scripts");
    std::fs::write(
        dir.join("witcherscript.toml"),
        format!("[content]\nscripts_root = \"{scripts_subdir}\"\n"),
    )
    .expect("write manifest");
}

#[test]
fn refresh_finds_manifests_at_multiple_depths() {
    let temp = LocalTempDir::new("ws_manifest_discovery_depths");
    write_manifest(temp.path(), "Mods/modA", "scripts");
    write_manifest(temp.path(), "Mods/modB/inner", "src");

    let backend = make_backend();
    backend.set_workspace_roots(vec![temp.path().to_path_buf()]);

    let changed = backend.refresh_manifest_legacy_dirs();
    assert!(changed, "first refresh must report a change");

    let dirs = backend.manifest_legacy_dirs.lock();
    assert_eq!(dirs.len(), 2, "expected two scripts_roots, got {dirs:?}");
}

#[test]
fn refresh_picks_up_gitignored_manifest() {
    let temp = LocalTempDir::new("ws_manifest_discovery_gitignored");
    std::fs::write(temp.path().join(".gitignore"), "Mods/\n").expect("write gitignore");
    write_manifest(temp.path(), "Mods/modA", "scripts");

    let backend = make_backend();
    backend.set_workspace_roots(vec![temp.path().to_path_buf()]);

    backend.refresh_manifest_legacy_dirs();
    assert_eq!(
        backend.manifest_legacy_dirs.lock().len(),
        1,
        "gitignored manifest must still be discovered"
    );
}

#[test]
fn refresh_honors_files_exclude() {
    let temp = LocalTempDir::new("ws_manifest_discovery_exclude");
    write_manifest(temp.path(), "keep", "scripts");
    write_manifest(temp.path(), "skip", "scripts");

    let backend = make_backend();
    backend.set_workspace_roots(vec![temp.path().to_path_buf()]);
    backend.update_config(|c| c.files_exclude = vec!["**/skip/**".to_string()]);

    backend.refresh_manifest_legacy_dirs();

    let dirs: Vec<_> = backend
        .manifest_legacy_dirs
        .lock()
        .values()
        .cloned()
        .collect();
    assert_eq!(
        dirs.len(),
        1,
        "expected only the kept manifest, got {dirs:?}"
    );
    assert!(dirs[0]
        .to_string_lossy()
        .replace('\\', "/")
        .contains("/keep/"));
}

#[test]
fn manifest_dirs_appear_in_effective_legacy_dirs() {
    let temp = LocalTempDir::new("ws_manifest_in_effective_legacy");
    write_manifest(temp.path(), "Mods/modA", "scripts");

    let backend = make_backend();
    backend.set_workspace_roots(vec![temp.path().to_path_buf()]);
    backend.refresh_manifest_legacy_dirs();

    let manifest_dir = temp.path().join("Mods/modA/scripts");
    let manifest_canon = manifest_dir
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.clone());

    let effective = backend.effective_legacy_dirs();
    assert!(
        effective.iter().any(|p| {
            let p = p.canonicalize().unwrap_or_else(|_| p.clone());
            p == manifest_canon
        }),
        "manifest scripts_root must be merged into effective_legacy_dirs; got {effective:?}"
    );
}
