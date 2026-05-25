use std::collections::HashMap;

use rstest::rstest;

use super::{collect_base_script_conflict_diagnostics, KIND};
use crate::diagnostics::{Severity, WorkspaceDiagnostic};
use crate::document::parse_document;
use crate::resolve::WorkspaceIndex;
use crate::test_support::TestDb;

const BASE_PLAYER_URI: &str = "file:///game/content/content0/scripts/game/r4Player.ws";
const WORKSPACE_PLAYER_URI: &str = "file:///mod/src/game/r4Player.ws";

fn collect(
    workspace_uri: &str,
    workspace_src: &str,
    base_uri: &str,
    base_src: &str,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let t = TestDb::new(&format!(
        "//- {}\n{}",
        workspace_uri.strip_prefix("file:///").unwrap(),
        workspace_src
    ))
    .with_base_doc(base_uri, base_src);
    collect_base_script_conflict_diagnostics(&t.workspace, &t.base, &[])
}

#[rstest]
#[case::same_class("class CR4Player {}\n", "class CR4Player {}\n", Some(1))]
#[case::same_function("function PlayerInit() {}\n", "function PlayerInit() {}\n", Some(1))]
#[case::same_state_same_owner(
    "state Combat in CR4Player {}\n",
    "state Combat in CR4Player {}\n",
    Some(1)
)]
#[case::same_state_different_owner(
    "state Combat in CR4Player {}\n",
    "state Combat in W3MonsterAI {}\n",
    None
)]
#[case::same_path_no_symbol_clash("class CMyThing {}\n", "class CR4Player {}\n", None)]
#[case::annotated_workspace_method(
    "@wrapMethod(CR4Player)\nfunction PlayerInit() {}\n",
    "function PlayerInit() {}\n",
    None
)]
fn detection_same_path(
    #[case] workspace_src: &str,
    #[case] base_src: &str,
    #[case] expected_count: Option<usize>,
) {
    let result = collect(
        WORKSPACE_PLAYER_URI,
        workspace_src,
        BASE_PLAYER_URI,
        base_src,
    );
    match expected_count {
        None => assert!(result.is_empty(), "expected no diagnostics, got {result:?}"),
        Some(n) => {
            let diags = result
                .get(WORKSPACE_PLAYER_URI)
                .expect("expected diagnostic on workspace file");
            assert_eq!(diags.len(), n, "diagnostic count mismatch");
            assert!(diags.iter().all(|d| d.kind == KIND), "kind mismatch");
        }
    }
}

#[rstest]
#[case::different_relpath("file:///mod/util/r4Player.ws", BASE_PLAYER_URI)]
#[case::different_basename("file:///mod/src/game/r4Other.ws", BASE_PLAYER_URI)]
#[case::directory_ending_in_base_subdir_name("file:///mod/src/r4game/r4Player.ws", BASE_PLAYER_URI)]
#[case::double_indexed_uri_self_match(BASE_PLAYER_URI, BASE_PLAYER_URI)]
#[case::base_file_without_scripts_segment(WORKSPACE_PLAYER_URI, "file:///game/r4Player.ws")]
fn detection_does_not_fire_by_path(#[case] workspace_uri: &str, #[case] base_uri: &str) {
    let result = collect(
        workspace_uri,
        "class CR4Player {}\n",
        base_uri,
        "class CR4Player {}\n",
    );
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn each_clashing_declaration_gets_its_own_diagnostic() {
    let result = collect(
        WORKSPACE_PLAYER_URI,
        "class CR4Player {}\nfunction PlayerInit() {}\n",
        BASE_PLAYER_URI,
        "class CR4Player {}\nfunction PlayerInit() {}\n",
    );
    let diags = result
        .get(WORKSPACE_PLAYER_URI)
        .expect("expected diagnostics on workspace file");
    assert_eq!(
        diags.len(),
        2,
        "one diagnostic per clashing declaration, got {}",
        diags.len()
    );
    assert!(diags.iter().all(|d| d.related.len() == 1));
    assert_eq!(diags[0].range.start.line, 0, "first diagnostic on class");
    assert_eq!(
        diags[1].range.start.line, 1,
        "second diagnostic on function"
    );
}

#[test]
fn workspace_file_under_a_legacy_dir_is_not_flagged() {
    let legacy_dir = std::env::temp_dir().join("bsc_legacy_skip_test");
    let ws_path = legacy_dir.join("game").join("r4Player.ws");
    let ws_uri = lsp_types::Url::from_file_path(&ws_path)
        .expect("absolute path -> url")
        .to_string();
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document(&ws_uri, &parse_document("class CR4Player {}\n").unwrap());
    let mut base = WorkspaceIndex::default();
    base.update_document(
        BASE_PLAYER_URI,
        &parse_document("class CR4Player {}\n").unwrap(),
    );

    assert!(
        !collect_base_script_conflict_diagnostics(&workspace, &base, &[]).is_empty(),
        "control: the file is flagged when its directory is not marked legacy",
    );
    assert!(
        collect_base_script_conflict_diagnostics(&workspace, &base, &[legacy_dir]).is_empty(),
        "a file under a configured legacy directory must not be flagged",
    );
}

#[test]
fn diagnostic_shape() {
    let result = collect(
        WORKSPACE_PLAYER_URI,
        "class CR4Player {}\n",
        BASE_PLAYER_URI,
        "class CR4Player {}\n",
    );
    let diags = result
        .get(WORKSPACE_PLAYER_URI)
        .expect("expected diagnostic on workspace file");
    let d = &diags[0];
    assert_eq!(d.kind, KIND);
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(
        d.range.start.line, 0,
        "diagnostic sits on the clashing declaration"
    );
    assert!(
        d.range.start.character > 0,
        "range is the declaration name, not file top: {:?}",
        d.range
    );
    assert!(
        d.message.contains("r4Player.ws"),
        "message should mention basename: {}",
        d.message
    );
    assert!(
        d.message.contains("CR4Player"),
        "message should mention the duplicate symbol: {}",
        d.message
    );
    assert!(
        d.message.contains("witcherscript.legacyScriptDirectories"),
        "message should mention config: {}",
        d.message
    );
    assert_eq!(d.related.len(), 1);
    assert_eq!(d.related[0].uri, BASE_PLAYER_URI);
    assert!(
        d.related[0].message.contains("CR4Player"),
        "related msg should mention symbol: {}",
        d.related[0].message
    );
}

#[test]
fn opened_base_script_under_client_uri_does_not_self_conflict() {
    let base_uri = "file:///c:/witcher3/content/content0/scripts/game/r4Player.ws";
    let client_uri = "file:///c%3A/witcher3/content/content0/scripts/game/r4Player.ws";
    let result = collect(
        client_uri,
        "class CR4Player {}\n",
        base_uri,
        "class CR4Player {}\n",
    );
    assert!(
        result.is_empty(),
        "an opened base script must not be flagged as replacing itself: {result:?}",
    );
}

#[test]
fn diagnostic_carries_mod_scripts_root_in_data() {
    let workspace_uri = "file:///c:/mymod/scripts/game/r4Player.ws";
    let base_uri = "file:///c:/witcher3/content/content0/scripts/game/r4Player.ws";
    let result = collect(
        workspace_uri,
        "class CR4Player {}\n",
        base_uri,
        "class CR4Player {}\n",
    );
    let diags = result
        .get(workspace_uri)
        .expect("expected diagnostic on workspace file");
    let data = diags[0]
        .data
        .as_ref()
        .expect("diagnostic should carry data");
    let directory = data
        .get("directory")
        .and_then(|v| v.as_str())
        .expect("data.directory should be a string");
    assert!(
        directory.ends_with("scripts"),
        "directory should be the mod scripts root, got '{directory}'",
    );
    assert!(
        directory.contains("mymod"),
        "directory should sit under the mod, got '{directory}'",
    );
}
