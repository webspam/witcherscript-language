use std::error::Error;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

pub fn collect_witcherscript_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();

    for path in paths {
        if path.is_file() {
            if is_witcherscript_file(path) {
                files.push(path.clone());
            }
            continue;
        }

        if path.is_dir() {
            for entry in WalkDir::new(path) {
                let entry = entry?;
                let entry_path = entry.path();
                if entry_path.is_file() && is_witcherscript_file(entry_path) {
                    files.push(entry_path.to_path_buf());
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
