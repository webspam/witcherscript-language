use super::collect_witcherscript_files;
use std::path::PathBuf;

#[test]
fn missing_path_is_an_error() {
    let missing = PathBuf::from("definitely/not/here.ws");
    assert!(collect_witcherscript_files(&[missing], &[]).is_err());
}
