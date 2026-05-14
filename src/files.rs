use std::error::Error;
use std::path::{Path, PathBuf};

use ignore::overrides::OverrideBuilder;
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
