use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ignore::overrides::{Override, OverrideBuilder};
use ignore::{Walk, WalkBuilder};
use lsp_types::Url;

pub fn canonical_uri(uri: &Url) -> Option<String> {
    let path = uri.to_file_path().ok()?;
    Url::from_file_path(path).ok().map(|u| u.to_string())
}

fn build_overrides(root: &Path, exclude_globs: &[String]) -> Result<Override, ignore::Error> {
    let mut builder = OverrideBuilder::new(root);
    for glob in exclude_globs {
        builder.add(&format!("!{glob}"))?;
    }
    builder.build()
}

fn standard_walker(path: &Path, overrides: Override) -> Walk {
    WalkBuilder::new(path)
        .require_git(false)
        .hidden(false)
        .overrides(overrides)
        .build()
}

pub fn collect_witcherscript_files(
    paths: &[PathBuf],
    exclude_globs: &[String],
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();

    for path in paths {
        if path.is_file() {
            if is_witcherscript_file(path) {
                files.push(path.clone());
            }
            continue;
        }

        if path.is_dir() {
            let overrides = build_overrides(path, exclude_globs)?;
            for entry in standard_walker(path, overrides) {
                let entry = entry?;
                if entry.file_type().is_some_and(|ft| ft.is_file())
                    && is_witcherscript_file(entry.path())
                {
                    files.push(entry.path().to_path_buf());
                }
            }
            continue;
        }

        return Err(format!("path does not exist: {}", path.display()).into());
    }

    files.sort();
    files.dedup();
    Ok(files)
}

pub fn is_witcherscript_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ws"))
}

pub fn read_script_file(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let words: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&words)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let words: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&words)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
    }
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub struct ExcludeFilter {
    per_root: Vec<RootFilter>,
}

struct RootFilter {
    root: PathBuf,
    exclude_globs: Vec<String>,
    overrides: Override,
}

impl ExcludeFilter {
    pub fn new(roots: &[PathBuf], exclude_globs: &[String]) -> Self {
        let per_root = roots
            .iter()
            .filter_map(|root| {
                let overrides = build_overrides(root, exclude_globs).ok()?;
                Some(RootFilter {
                    root: root.clone(),
                    exclude_globs: exclude_globs.to_vec(),
                    overrides,
                })
            })
            .collect();
        Self { per_root }
    }

    pub fn matches(&self, path: &Path) -> bool {
        for f in &self.per_root {
            if let Ok(rel) = path.strip_prefix(&f.root) {
                if f.overrides.matched(rel, false).is_ignore() {
                    return true;
                }
            }
        }
        // WalkBuilder can't probe gitignore for a path that isn't on disk (e.g. synthetic test paths).
        if !path.is_file() {
            return false;
        }
        for f in &self.per_root {
            if !path.starts_with(&f.root) {
                continue;
            }
            let Ok(overrides) = build_overrides(&f.root, &f.exclude_globs) else {
                continue;
            };
            // Walk from the root down only the target's lineage so gitignore can prune any ignored ancestor.
            let target = path.to_path_buf();
            let walker = WalkBuilder::new(&f.root)
                .require_git(false)
                .hidden(false)
                .overrides(overrides)
                .filter_entry(move |e| target.starts_with(e.path()))
                .build();
            let visible = walker.filter_map(Result::ok).any(|e| e.path() == path);
            return !visible;
        }
        false
    }
}

#[cfg(test)]
mod tests;
