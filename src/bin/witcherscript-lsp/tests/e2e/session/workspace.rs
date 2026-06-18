use std::fs;
use std::path::{Path, PathBuf};

use lsp_types::{Position, Url};
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::fixture::Fixture;
use crate::tests::support::LocalTempDir;

pub(crate) struct LoadedWorkspace {
    root: PathBuf,
    _temp: LocalTempDir,
    files: Vec<LoadedFile>,
    workspace_roots: Vec<PathBuf>,
    config_overrides: Vec<(String, Value)>,
}

pub(crate) struct LoadedFile {
    pub(crate) rel: String,
    pub(crate) uri: Url,
    pub(crate) text: String,
    pub(crate) cursor: Option<Position>,
}

#[derive(Deserialize)]
struct WorkspaceManifest {
    #[serde(default = "default_roots")]
    roots: Vec<String>,
    game_dir: Option<String>,
    base_scripts_dir: Option<String>,
    #[serde(default)]
    additional_dirs: Vec<String>,
    #[serde(default)]
    legacy_dirs: Vec<String>,
    diagnostics_scope: Option<String>,
}

fn default_roots() -> Vec<String> {
    vec![".".to_string()]
}

fn workspaces_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("workspaces")
}

impl LoadedWorkspace {
    pub(crate) fn materialize(fixture_dir_name: &str) -> Self {
        let src = workspaces_root().join(fixture_dir_name);
        let manifest_text = fs::read_to_string(src.join("workspace.toml"))
            .unwrap_or_else(|e| panic!("read workspace.toml for {fixture_dir_name}: {e}"));
        let manifest: WorkspaceManifest = toml::from_str(&manifest_text)
            .unwrap_or_else(|e| panic!("parse workspace.toml for {fixture_dir_name}: {e}"));

        let temp = LocalTempDir::under_target(&format!("ws-{fixture_dir_name}"));
        let root = temp.path().to_path_buf();

        let mut sources = Vec::new();
        collect_files(&src, &mut sources);
        let mut files = Vec::new();
        for source_path in sources {
            let rel = source_path
                .strip_prefix(&src)
                .expect("collected file lives under the fixture dir");
            let rel_fwd = rel.to_string_lossy().replace('\\', "/");
            let dst = root.join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).expect("mkdir temp parent");
            }
            let is_ws = source_path.extension().is_some_and(|e| e == "ws");
            if is_ws && is_under_roots(&rel_fwd, &manifest.roots) {
                materialize_marker_file(&source_path, &dst, &rel_fwd, &mut files);
            } else {
                fs::copy(&source_path, &dst).expect("copy fixture file");
            }
        }
        files.sort_by(|a, b| a.rel.cmp(&b.rel));

        let workspace_roots = manifest
            .roots
            .iter()
            .map(|r| resolve_under(&root, r))
            .collect();
        let config_overrides = build_config_overrides(&root, &manifest);

        Self {
            root,
            _temp: temp,
            files,
            workspace_roots,
            config_overrides,
        }
    }

    pub(crate) fn files(&self) -> &[LoadedFile] {
        &self.files
    }

    pub(crate) fn workspace_roots(&self) -> &[PathBuf] {
        &self.workspace_roots
    }

    pub(crate) fn config_overrides(&self) -> &[(String, Value)] {
        &self.config_overrides
    }

    // Workspace-relative, forward-slashed identity; an URL outside the tree is tagged so a leak is visible, not silently nondeterministic.
    pub(crate) fn relativize(&self, url: &Url) -> String {
        let Ok(path) = url.to_file_path() else {
            return url.as_str().to_string();
        };
        let path_str = path.to_string_lossy().replace('\\', "/");
        let root_str = self.root.to_string_lossy().replace('\\', "/");
        match strip_prefix_ci(&path_str, &root_str) {
            Some(rest) => rest.trim_start_matches('/').to_string(),
            None => format!("EXTERNAL:{path_str}"),
        }
    }

    // Hover/diagnostic text embeds absolute file URLs of the temp dir; rewrite the root prefix so snapshots are machine-independent.
    pub(crate) fn redact_urls(&self, text: &str) -> String {
        let Ok(root_url) = Url::from_directory_path(&self.root) else {
            return text.to_string();
        };
        replace_ci(text, root_url.as_str(), "file:///WORKSPACE/")
    }
}

fn materialize_marker_file(source: &Path, dst: &Path, rel_fwd: &str, files: &mut Vec<LoadedFile>) {
    let raw = fs::read_to_string(source).expect("read ws fixture file");
    let fixture = Fixture::parse(&raw);
    assert_eq!(
        fixture.files.len(),
        1,
        "disk fixture {rel_fwd} must not contain //- file splits"
    );
    let stripped = fixture.files[0].text.clone();
    fs::write(dst, &stripped).expect("write stripped ws file");
    files.push(LoadedFile {
        rel: rel_fwd.to_string(),
        uri: Url::from_file_path(dst).expect("temp path -> URI"),
        text: stripped,
        cursor: fixture.cursor.map(|(_, p)| p),
    });
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read fixture dir") {
        let path = entry.expect("fixture dir entry").path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn is_under_roots(rel_fwd: &str, roots: &[String]) -> bool {
    roots
        .iter()
        .any(|r| r == "." || rel_fwd == r || rel_fwd.starts_with(&format!("{r}/")))
}

fn resolve_under(root: &Path, rel: &str) -> PathBuf {
    if rel == "." {
        root.to_path_buf()
    } else {
        root.join(rel)
    }
}

fn build_config_overrides(root: &Path, manifest: &WorkspaceManifest) -> Vec<(String, Value)> {
    let abs = |rel: &str| resolve_under(root, rel).to_string_lossy().to_string();
    let mut overrides = Vec::new();
    if let Some(game) = &manifest.game_dir {
        overrides.push(("witcherscript.gameDirectory".to_string(), json!(abs(game))));
    }
    if let Some(base) = &manifest.base_scripts_dir {
        overrides.push((
            "witcherscript.baseScriptsDirectory".to_string(),
            json!(abs(base)),
        ));
    }
    if !manifest.additional_dirs.is_empty() {
        let dirs: Vec<String> = manifest.additional_dirs.iter().map(|d| abs(d)).collect();
        overrides.push((
            "witcherscript.additionalScriptDirectories".to_string(),
            json!(dirs),
        ));
    }
    if !manifest.legacy_dirs.is_empty() {
        let dirs: Vec<String> = manifest.legacy_dirs.iter().map(|d| abs(d)).collect();
        overrides.push((
            "witcherscript.legacyScriptDirectories".to_string(),
            json!(dirs),
        ));
    }
    if let Some(scope) = &manifest.diagnostics_scope {
        overrides.push(("witcherscript.diagnostics.scope".to_string(), json!(scope)));
    }
    overrides
}

// Windows file URLs can come back with a different drive-letter case than the temp root; match the prefix case-insensitively but keep the suffix verbatim.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn replace_ci(haystack: &str, needle: &str, replacement: &str) -> String {
    let haystack_low = haystack.to_ascii_lowercase();
    let needle_low = needle.to_ascii_lowercase();
    let mut result = String::with_capacity(haystack.len());
    let mut last = 0;
    while let Some(found) = haystack_low[last..].find(&needle_low) {
        let idx = last + found;
        result.push_str(&haystack[last..idx]);
        result.push_str(replacement);
        last = idx + needle.len();
    }
    result.push_str(&haystack[last..]);
    result
}
