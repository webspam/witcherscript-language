use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use serde::Deserialize;
use tracing::{trace, warn};
use witcherscript_language::files::build_overrides;

pub(crate) const MANIFEST_FILENAME: &str = "witcherscript.toml";

// Spec default: see https://spontancombust.github.io/witcherscript-ide/user-manual/project-system/#manifest-format
const DEFAULT_SCRIPTS_ROOT: &str = "./scripts";

#[derive(Debug, Deserialize)]
struct Manifest {
    content: Option<ContentSection>,
}

#[derive(Debug, Deserialize)]
struct ContentSection {
    scripts_root: Option<String>,
}

pub(crate) fn parse_manifest(toml_path: &Path) -> Option<PathBuf> {
    let text = match fs::read_to_string(toml_path) {
        Ok(t) => t,
        Err(err) => {
            warn!(path = %toml_path.display(), error = %err, "failed to read witcherscript.toml");
            return None;
        }
    };
    let manifest: Manifest = match toml::from_str(&text) {
        Ok(m) => m,
        Err(err) => {
            warn!(path = %toml_path.display(), error = %err, "failed to parse witcherscript.toml");
            return None;
        }
    };

    let scripts_root_str = manifest
        .content
        .as_ref()
        .and_then(|c| c.scripts_root.as_deref())
        .unwrap_or(DEFAULT_SCRIPTS_ROOT);

    let Some(parent) = toml_path.parent() else {
        warn!(path = %toml_path.display(), "witcherscript.toml has no parent directory; skipping");
        return None;
    };
    let resolved = parent.join(scripts_root_str);
    if !resolved.is_dir() {
        warn!(
            path = %toml_path.display(),
            scripts_root = %resolved.display(),
            "witcherscript.toml scripts_root is not an existing directory; skipping"
        );
        return None;
    }
    trace!(
        manifest = %toml_path.display(),
        scripts_root = %resolved.display(),
        "loaded witcherscript.toml manifest"
    );
    Some(resolved)
}

/// Collect `witcherscript.toml` files. Honors `files.exclude`; ignores `.gitignore`. Not for hot paths.
pub(crate) fn discover_manifests(roots: &[PathBuf], exclude_globs: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    trace!(roots = ?roots, "scanning workspace roots for witcherscript.toml");
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let overrides = match build_overrides(root, exclude_globs) {
            Ok(o) => o,
            Err(err) => {
                warn!(root = %root.display(), error = %err, "failed to build exclude overrides for manifest discovery");
                continue;
            }
        };
        let walker = WalkBuilder::new(root)
            .require_git(false)
            .hidden(false)
            .git_ignore(false)
            .git_exclude(false)
            .git_global(false)
            .overrides(overrides)
            .build();
        for entry in walker.filter_map(Result::ok) {
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            if entry.file_name() == MANIFEST_FILENAME {
                out.push(entry.path().to_path_buf());
            }
        }
    }
    out.sort();
    out.dedup();
    trace!(count = out.len(), "discovered witcherscript.toml manifests");
    out
}

#[cfg(test)]
mod tests;
