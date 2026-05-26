use std::fs;
use std::path::{Path, PathBuf};

use super::{discover_manifests, parse_manifest, MANIFEST_FILENAME};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::current_dir()
            .expect("cwd")
            .join("target/tmp/manifest_unit")
            .join(format!(
                "{}-{}-{}",
                label,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&dir).expect("create temp dir");
        Self { path: dir }
    }
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write(dir: &Path, rel: &str, contents: &str) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(&path, contents).expect("write file");
    path
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

#[test]
fn parse_manifest_returns_resolved_scripts_root() {
    let tmp = TempDir::new("parse-explicit");
    let toml = write(
        tmp.path(),
        "witcherscript.toml",
        r#"[content]
name = "modX"
version = "1.0.0"
scripts_root = "scripts"
"#,
    );
    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).unwrap();

    let resolved = parse_manifest(&toml).expect("should resolve");
    assert_eq!(canon(&resolved), canon(&scripts));
}

#[test]
fn parse_manifest_uses_default_when_field_missing() {
    let tmp = TempDir::new("parse-default");
    let toml = write(
        tmp.path(),
        "witcherscript.toml",
        r#"[content]
name = "modX"
version = "1.0.0"
"#,
    );
    let default_scripts = tmp.path().join("scripts");
    fs::create_dir_all(&default_scripts).unwrap();

    let resolved = parse_manifest(&toml).expect("should resolve to ./scripts");
    assert_eq!(canon(&resolved), canon(&default_scripts));
}

#[test]
fn parse_manifest_returns_none_for_malformed_toml() {
    let tmp = TempDir::new("parse-bad");
    let toml = write(tmp.path(), "witcherscript.toml", "this is not toml ::: !");
    assert!(parse_manifest(&toml).is_none());
}

#[test]
fn parse_manifest_returns_none_when_scripts_root_missing_on_disk() {
    let tmp = TempDir::new("parse-missing-dir");
    let toml = write(
        tmp.path(),
        "witcherscript.toml",
        r#"[content]
scripts_root = "does_not_exist"
"#,
    );
    assert!(parse_manifest(&toml).is_none());
}

#[test]
fn parse_manifest_ignores_other_fields_and_dependencies() {
    let tmp = TempDir::new("parse-extra-fields");
    let toml = write(
        tmp.path(),
        "witcherscript.toml",
        r#"[content]
name = "modX"
description = ""
version = "1.0.0"
authors = []
game_version = "4.04"
scripts_root = "scripts"

[dependencies]
content0 = true
modOther = { path = "../other" }
"#,
    );
    fs::create_dir_all(tmp.path().join("scripts")).unwrap();
    assert!(parse_manifest(&toml).is_some());
}

#[test]
fn discover_manifests_recurses_and_finds_nested() {
    let tmp = TempDir::new("disc-nested");
    write(
        tmp.path(),
        "Mods/modA/witcherscript.toml",
        "[content]\nscripts_root = \"scripts\"\n",
    );
    write(
        tmp.path(),
        "Mods/modB/inner/witcherscript.toml",
        "[content]\nscripts_root = \"scripts\"\n",
    );
    let found = discover_manifests(&[tmp.path().to_path_buf()], &[]);
    assert_eq!(
        found.len(),
        2,
        "expected two nested manifests, got {found:?}"
    );
    assert!(found
        .iter()
        .all(|p| p.file_name().unwrap() == MANIFEST_FILENAME));
}

#[test]
fn discover_manifests_honors_files_exclude() {
    let tmp = TempDir::new("disc-exclude");
    write(
        tmp.path(),
        "keep/witcherscript.toml",
        "[content]\nscripts_root = \"scripts\"\n",
    );
    write(
        tmp.path(),
        "skip/witcherscript.toml",
        "[content]\nscripts_root = \"scripts\"\n",
    );
    let found = discover_manifests(&[tmp.path().to_path_buf()], &["**/skip/**".to_string()]);
    assert_eq!(found.len(), 1);
    assert!(found[0]
        .to_string_lossy()
        .replace('\\', "/")
        .contains("/keep/"));
}

#[test]
fn discover_manifests_finds_gitignored_files() {
    let tmp = TempDir::new("disc-gitignored");
    // Pretend this is a git repo by writing a .gitignore that excludes everything below.
    write(tmp.path(), ".gitignore", "Mods/\n");
    write(
        tmp.path(),
        "Mods/mod1/witcherscript.toml",
        "[content]\nscripts_root = \"scripts\"\n",
    );
    let found = discover_manifests(&[tmp.path().to_path_buf()], &[]);
    assert_eq!(
        found.len(),
        1,
        "gitignored manifests must still be found; got {found:?}"
    );
}
