use std::path::Path;

use lsp_types::Url;

use super::harness::LspClientBuilder;
use crate::tests::support::LocalTempDir;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, contents).expect("write file");
}

#[tokio::test]
async fn open_gitignored_duplicate_never_conflicts_with_project_file() {
    let temp = LocalTempDir::new("e2e_excluded_no_conflict");
    write(&temp.path().join(".gitignore"), "build/\n");
    write(&temp.path().join("Real.ws"), "class CDup {}\n");
    write(&temp.path().join("build/Dup.ws"), "class CDup {}\n");

    let real_url = Url::from_file_path(temp.path().join("Real.ws")).expect("real url");
    let build_url = Url::from_file_path(temp.path().join("build/Dup.ws")).expect("build url");

    let mut client = LspClientBuilder::new().root(temp.path()).spawn().await;

    assert!(
        client.pull_diagnostics(&real_url).await.is_empty(),
        "the project file must be clean before the gitignored copy is touched",
    );

    client.open(&build_url, "class CDup {}\n").await;
    assert!(
        client.pull_diagnostics(&real_url).await.is_empty(),
        "opening the gitignored copy must not make the project file conflict with it",
    );

    client
        .change_full(&build_url, 2, "class CDup { function f() {} }\n")
        .await;
    assert!(
        client.pull_diagnostics(&real_url).await.is_empty(),
        "editing the gitignored copy must not make the project file conflict with it",
    );

    client.close(&build_url).await;
    assert!(
        client.pull_diagnostics(&real_url).await.is_empty(),
        "closing the gitignored copy must not leave the project file conflicting with it",
    );
}
