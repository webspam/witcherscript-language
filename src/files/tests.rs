use super::{ExcludeFilter, collect_witcherscript_files};
use std::path::{Path, PathBuf};

#[test]
fn missing_path_is_an_error() {
    let missing = PathBuf::from("definitely/not/here.ws");
    assert!(collect_witcherscript_files(&[missing], &[]).is_err());
}

struct ScopedDir(PathBuf);

impl ScopedDir {
    fn new(name: &str) -> Self {
        let path = PathBuf::from("target/tmp").join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("mkdir tempdir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScopedDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, contents).expect("write file");
}

#[test]
fn exclude_filter_honors_gitignore_for_real_paths() {
    let dir = ScopedDir::new("exclude_filter_gitignore");
    write(&dir.path().join(".gitignore"), "build/\n");
    let kept = dir.path().join("src/keep.ws");
    let ignored = dir.path().join("build/skip.ws");
    write(&kept, "");
    write(&ignored, "");

    let filter = ExcludeFilter::new(&[dir.path().to_path_buf()], &[]);
    assert!(!filter.matches(&kept), "non-ignored file must not match");
    assert!(filter.matches(&ignored), "gitignored file must match");
}

#[test]
fn exclude_filter_honors_user_globs_on_synthetic_paths() {
    let root = if cfg!(windows) {
        PathBuf::from(r"C:\fake-root")
    } else {
        PathBuf::from("/fake-root")
    };
    let filter = ExcludeFilter::new(std::slice::from_ref(&root), &["vendor/**".to_string()]);
    assert!(filter.matches(&root.join("vendor/lib.ws")));
    assert!(!filter.matches(&root.join("src/main.ws")));
}
