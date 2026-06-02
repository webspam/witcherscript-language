use std::collections::HashMap;
use std::path::PathBuf;

use lsp_types::Url;
use rstest::rstest;
use witcherscript_language::files::canonical_uri;

use crate::file_scope::{classify_file_scope, FileScope};
use crate::file_scope_status::FileScopeStatusParams;

fn dir(rel: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!("C:\\{}", rel.replace('/', "\\")))
    } else {
        PathBuf::from(format!("/{rel}"))
    }
}

fn url(rel: &str) -> Url {
    Url::from_file_path(dir(rel)).expect("file url")
}

fn override_map() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        canonical_uri(&url("legacy/game/r4Player.ws")),
        "game/r4Player.ws".to_string(),
    );
    m
}

#[rstest]
#[case::under_a_workspace_root(
    url("proj/Foo.ws"),
    vec![dir("proj")],
    vec![],
    HashMap::new(),
    None,
    vec![],
    FileScope::InProject,
)]
#[case::no_workspace_folders_open(
    url("anywhere/Foo.ws"),
    vec![],
    vec![],
    HashMap::new(),
    None,
    vec![],
    FileScope::SingleFile,
)]
#[case::workspace_open_but_file_outside_every_root(
    url("outside/Foo.ws"),
    vec![dir("proj")],
    vec![],
    HashMap::new(),
    None,
    vec![],
    FileScope::OutOfScope,
)]
#[case::legacy_dir_recorded_as_a_base_override(
    url("legacy/game/r4Player.ws"),
    vec![dir("proj")],
    vec![dir("legacy")],
    override_map(),
    None,
    vec![],
    FileScope::LegacyOverride,
)]
#[case::legacy_dir_not_an_override(
    url("legacy/game/New.ws"),
    vec![dir("proj")],
    vec![dir("legacy")],
    HashMap::new(),
    None,
    vec![],
    FileScope::LegacyNew,
)]
#[case::under_the_game_directory(
    url("game/content/content0/scripts/game/r4Player.ws"),
    vec![dir("proj")],
    vec![],
    HashMap::new(),
    Some(dir("game")),
    vec![],
    FileScope::AdditionalBase,
)]
#[case::under_an_additional_script_directory(
    url("extra/Lib.ws"),
    vec![dir("proj")],
    vec![],
    HashMap::new(),
    None,
    vec![dir("extra")],
    FileScope::AdditionalBase,
)]
#[case::legacy_precedence_beats_additional_for_the_same_dir(
    url("shared/game/r4Player.ws"),
    vec![dir("proj")],
    vec![dir("shared")],
    HashMap::new(),
    None,
    vec![dir("shared")],
    FileScope::LegacyNew,
)]
#[case::workspace_root_nested_over_a_legacy_dir_wins(
    url("proj/legacy/game/r4Player.ws"),
    vec![dir("proj")],
    vec![dir("proj/legacy")],
    HashMap::new(),
    None,
    vec![],
    FileScope::InProject,
)]
fn classifies_file_scope(
    #[case] uri: Url,
    #[case] roots: Vec<PathBuf>,
    #[case] legacy: Vec<PathBuf>,
    #[case] replacements: HashMap<String, String>,
    #[case] game: Option<PathBuf>,
    #[case] additional: Vec<PathBuf>,
    #[case] expected: FileScope,
) {
    let got = classify_file_scope(
        &uri,
        &roots,
        &legacy,
        &replacements,
        game.as_deref(),
        &additional,
    );
    assert_eq!(got, expected, "scope mismatch");
}

#[test]
fn file_scope_status_params_serializes_camel_case() {
    let loose = FileScopeStatusParams {
        uri: "file:///x.ws".to_string(),
        scope: FileScope::OutOfScope,
        replaced_script_path: None,
    };
    let value = serde_json::to_value(&loose).expect("serialize");
    assert_eq!(value["uri"], "file:///x.ws");
    assert_eq!(value["scope"], "outOfScope");
    assert!(
        value.get("replacedScriptPath").is_none(),
        "an absent replaced path must be omitted from the payload",
    );

    let override_status = FileScopeStatusParams {
        uri: "file:///y.ws".to_string(),
        scope: FileScope::LegacyOverride,
        replaced_script_path: Some("game/r4Player.ws".to_string()),
    };
    let value = serde_json::to_value(&override_status).expect("serialize");
    assert_eq!(value["scope"], "legacyOverride");
    assert_eq!(value["replacedScriptPath"], "game/r4Player.ws");
}
