use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ignore::overrides::{Override, OverrideBuilder};
use ignore::WalkBuilder;

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
            let mut overrides = OverrideBuilder::new(path);
            for glob in exclude_globs {
                overrides.add(&format!("!{glob}"))?;
            }
            let overrides = overrides.build()?;

            let walker = WalkBuilder::new(path)
                .require_git(false)
                .hidden(false)
                .overrides(overrides)
                .build();
            for entry in walker {
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
    per_root: Vec<(PathBuf, Override)>,
}

impl ExcludeFilter {
    pub fn new(roots: &[PathBuf], exclude_globs: &[String]) -> Self {
        if exclude_globs.is_empty() {
            return Self {
                per_root: Vec::new(),
            };
        }
        let per_root = roots
            .iter()
            .filter_map(|root| {
                let mut builder = OverrideBuilder::new(root);
                for glob in exclude_globs {
                    builder.add(&format!("!{glob}")).ok()?;
                }
                Some((root.clone(), builder.build().ok()?))
            })
            .collect();
        Self { per_root }
    }

    pub fn matches(&self, path: &Path) -> bool {
        for (root, overrides) in &self.per_root {
            if let Ok(rel) = path.strip_prefix(root) {
                if overrides.matched(rel, false).is_ignore() {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::collect_witcherscript_files;
    use std::path::PathBuf;

    #[test]
    fn missing_path_is_an_error() {
        let missing = PathBuf::from("definitely/not/here.ws");
        assert!(collect_witcherscript_files(&[missing], &[]).is_err());
    }
}
