use std::collections::HashMap;

use super::{collect_base_script_conflict_diagnostics, KIND};
use crate::diagnostics::{Severity, WorkspaceDiagnostic};
use crate::document::parse_document;
use crate::resolve::WorkspaceIndex;

const BASE_PLAYER_URI: &str = "file:///game/content/content0/scripts/game/r4Player.ws";
const WORKSPACE_PLAYER_URI: &str = "file:///mod/src/game/r4Player.ws";

fn build_index(docs: &[(&str, &str)]) -> WorkspaceIndex {
    let mut idx = WorkspaceIndex::default();
    for (uri, src) in docs {
        let doc = parse_document(*src).expect("parse should succeed");
        idx.update_document(*uri, &doc);
    }
    idx
}

fn collect(
    workspace_docs: &[(&str, &str)],
    base_docs: &[(&str, &str)],
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let workspace = build_index(workspace_docs);
    let base = build_index(base_docs);
    collect_base_script_conflict_diagnostics(&workspace, &base, &[])
}

#[test]
fn detection_table() {
    struct Case {
        name: &'static str,
        workspace: Vec<(&'static str, &'static str)>,
        base: Vec<(&'static str, &'static str)>,
        expected_uri: Option<&'static str>,
        expected_count: usize,
    }
    let cases = vec![
        Case {
            name: "same basename and relpath + same class fires",
            workspace: vec![(WORKSPACE_PLAYER_URI, "class CR4Player {}\n")],
            base: vec![(BASE_PLAYER_URI, "class CR4Player {}\n")],
            expected_uri: Some(WORKSPACE_PLAYER_URI),
            expected_count: 1,
        },
        Case {
            name: "same basename and relpath + same function fires",
            workspace: vec![(WORKSPACE_PLAYER_URI, "function PlayerInit() {}\n")],
            base: vec![(BASE_PLAYER_URI, "function PlayerInit() {}\n")],
            expected_uri: Some(WORKSPACE_PLAYER_URI),
            expected_count: 1,
        },
        Case {
            name: "same state same owner fires",
            workspace: vec![(WORKSPACE_PLAYER_URI, "state Combat in CR4Player {}\n")],
            base: vec![(BASE_PLAYER_URI, "state Combat in CR4Player {}\n")],
            expected_uri: Some(WORKSPACE_PLAYER_URI),
            expected_count: 1,
        },
        Case {
            name: "same state different owner does not fire",
            workspace: vec![(WORKSPACE_PLAYER_URI, "state Combat in CR4Player {}\n")],
            base: vec![(BASE_PLAYER_URI, "state Combat in W3MonsterAI {}\n")],
            expected_uri: None,
            expected_count: 0,
        },
        Case {
            name: "different relpath does not fire",
            workspace: vec![("file:///mod/util/r4Player.ws", "class CR4Player {}\n")],
            base: vec![(BASE_PLAYER_URI, "class CR4Player {}\n")],
            expected_uri: None,
            expected_count: 0,
        },
        Case {
            name: "different basename does not fire",
            workspace: vec![("file:///mod/src/game/r4Other.ws", "class CR4Player {}\n")],
            base: vec![(BASE_PLAYER_URI, "class CR4Player {}\n")],
            expected_uri: None,
            expected_count: 0,
        },
        Case {
            name: "directory ending in the base subdir name does not fire",
            workspace: vec![("file:///mod/src/r4game/r4Player.ws", "class CR4Player {}\n")],
            base: vec![(BASE_PLAYER_URI, "class CR4Player {}\n")],
            expected_uri: None,
            expected_count: 0,
        },
        Case {
            name: "same path no symbol clash does not fire",
            workspace: vec![(WORKSPACE_PLAYER_URI, "class CMyThing {}\n")],
            base: vec![(BASE_PLAYER_URI, "class CR4Player {}\n")],
            expected_uri: None,
            expected_count: 0,
        },
        Case {
            name: "annotated workspace method does not fire",
            workspace: vec![(
                WORKSPACE_PLAYER_URI,
                "@wrapMethod(CR4Player)\nfunction PlayerInit() {}\n",
            )],
            base: vec![(BASE_PLAYER_URI, "function PlayerInit() {}\n")],
            expected_uri: None,
            expected_count: 0,
        },
        Case {
            name: "double-indexed uri does not fire (self-match)",
            workspace: vec![(BASE_PLAYER_URI, "class CR4Player {}\n")],
            base: vec![(BASE_PLAYER_URI, "class CR4Player {}\n")],
            expected_uri: None,
            expected_count: 0,
        },
        Case {
            name: "base file without /scripts/ segment is ignored",
            workspace: vec![(WORKSPACE_PLAYER_URI, "class CR4Player {}\n")],
            base: vec![("file:///game/r4Player.ws", "class CR4Player {}\n")],
            expected_uri: None,
            expected_count: 0,
        },
    ];

    for c in cases {
        let result = collect(&c.workspace, &c.base);
        match c.expected_uri {
            None => {
                assert!(
                    result.is_empty(),
                    "case '{}': expected no diagnostics, got {:?}",
                    c.name,
                    result
                );
            }
            Some(uri) => {
                let diags = result
                    .get(uri)
                    .unwrap_or_else(|| panic!("case '{}': expected diagnostic on {}", c.name, uri));
                assert_eq!(
                    diags.len(),
                    c.expected_count,
                    "case '{}': diagnostic count mismatch",
                    c.name
                );
                assert!(
                    diags.iter().all(|d| d.kind == KIND),
                    "case '{}': kind mismatch",
                    c.name
                );
            }
        }
    }
}

#[test]
fn each_clashing_declaration_gets_its_own_diagnostic() {
    let result = collect(
        &[(
            WORKSPACE_PLAYER_URI,
            "class CR4Player {}\nfunction PlayerInit() {}\n",
        )],
        &[(
            BASE_PLAYER_URI,
            "class CR4Player {}\nfunction PlayerInit() {}\n",
        )],
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
    let workspace = build_index(&[(ws_uri.as_str(), "class CR4Player {}\n")]);
    let base = build_index(&[(BASE_PLAYER_URI, "class CR4Player {}\n")]);

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
        &[(WORKSPACE_PLAYER_URI, "class CR4Player {}\n")],
        &[(BASE_PLAYER_URI, "class CR4Player {}\n")],
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
        &[(client_uri, "class CR4Player {}\n")],
        &[(base_uri, "class CR4Player {}\n")],
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
        &[(workspace_uri, "class CR4Player {}\n")],
        &[(base_uri, "class CR4Player {}\n")],
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
