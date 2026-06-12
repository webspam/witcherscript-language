use lsp_types::{
    OneOf, SymbolKind, Url, WorkspaceSymbol, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use super::legacy_helpers::write_script;
use crate::backend::Backend;
use crate::tests::support::{LocalTempDir, make_backend};

fn query(backend: &Backend, q: &str) -> Vec<WorkspaceSymbol> {
    let response = backend
        ._workspace_symbol(WorkspaceSymbolParams {
            query: q.to_string(),
            ..Default::default()
        })
        .expect("workspace symbol handler ok")
        .expect("workspace symbol response present");
    let WorkspaceSymbolResponse::Nested(symbols) = response else {
        panic!("expected nested workspace symbol response");
    };
    symbols
}

#[tokio::test]
async fn returns_project_symbols_with_kind_location_and_container() {
    let temp = LocalTempDir::new("ws_workspace_symbol_project");
    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let script = write_script(
        &project_dir,
        "MyMod.ws",
        "class CMyMod {\n  function DoMod() {}\n}\n",
    );
    let script_url = Url::from_file_path(&script).unwrap();

    let backend = make_backend();
    backend.set_workspace_roots(vec![project_dir]);
    backend.index_workspace().await;

    let class = query(&backend, "CMyMod")
        .into_iter()
        .find(|s| s.name == "CMyMod")
        .expect("project class should be found");
    assert_eq!(
        class.kind,
        SymbolKind::CLASS,
        "class should map to CLASS kind"
    );
    match class.location {
        OneOf::Left(location) => {
            assert_eq!(
                location.uri, script_url,
                "location should point at the project file"
            );
        }
        OneOf::Right(_) => panic!("expected a resolved Location"),
    }

    let method = query(&backend, "DoMod")
        .into_iter()
        .find(|s| s.name == "DoMod")
        .expect("project method should be found");
    assert_eq!(
        method.kind,
        SymbolKind::METHOD,
        "method should map to METHOD kind"
    );
    assert_eq!(
        method.container_name.as_deref(),
        Some("CMyMod"),
        "method should carry its container name"
    );
}

#[tokio::test]
async fn excludes_loose_files() {
    let temp = LocalTempDir::new("ws_workspace_symbol_loose");
    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    write_script(&project_dir, "MyMod.ws", "class CMyMod {}\n");

    let backend = make_backend();
    backend.set_workspace_roots(vec![project_dir]);
    backend.index_workspace().await;

    let loose_path = write_script(temp.path(), "outside/Loose.ws", "class CLooseOnly {}\n");
    let loose_url = Url::from_file_path(&loose_path).unwrap();
    backend.update_open_document(loose_url, "class CLooseOnly {}\n".to_string());

    assert!(
        query(&backend, "CLooseOnly").is_empty(),
        "loose (out-of-project) symbols must not appear in project-wide search",
    );
}
