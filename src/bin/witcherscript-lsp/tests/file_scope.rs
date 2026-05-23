use std::collections::HashMap;
use std::path::PathBuf;

use lsp_types::Url;
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

#[test]
fn classifies_file_scope() {
    struct Case {
        name: &'static str,
        uri: Url,
        roots: Vec<PathBuf>,
        legacy: Vec<PathBuf>,
        replacements: HashMap<String, String>,
        game: Option<PathBuf>,
        additional: Vec<PathBuf>,
        expected: FileScope,
    }

    let override_uri = url("legacy/game/r4Player.ws");
    let mut override_map = HashMap::new();
    override_map.insert(
        canonical_uri(&override_uri).expect("canonical override uri"),
        "game/r4Player.ws".to_string(),
    );

    let cases = [
        Case {
            name: "under a workspace root",
            uri: url("proj/Foo.ws"),
            roots: vec![dir("proj")],
            legacy: vec![],
            replacements: HashMap::new(),
            game: None,
            additional: vec![],
            expected: FileScope::InProject,
        },
        Case {
            name: "no workspace folders open",
            uri: url("anywhere/Foo.ws"),
            roots: vec![],
            legacy: vec![],
            replacements: HashMap::new(),
            game: None,
            additional: vec![],
            expected: FileScope::SingleFile,
        },
        Case {
            name: "workspace open but file outside every root",
            uri: url("outside/Foo.ws"),
            roots: vec![dir("proj")],
            legacy: vec![],
            replacements: HashMap::new(),
            game: None,
            additional: vec![],
            expected: FileScope::OutOfScope,
        },
        Case {
            name: "legacy dir, recorded as a base override",
            uri: override_uri.clone(),
            roots: vec![dir("proj")],
            legacy: vec![dir("legacy")],
            replacements: override_map.clone(),
            game: None,
            additional: vec![],
            expected: FileScope::LegacyOverride,
        },
        Case {
            name: "legacy dir, not an override",
            uri: url("legacy/game/New.ws"),
            roots: vec![dir("proj")],
            legacy: vec![dir("legacy")],
            replacements: HashMap::new(),
            game: None,
            additional: vec![],
            expected: FileScope::LegacyNew,
        },
        Case {
            name: "under the game directory",
            uri: url("game/content/content0/scripts/game/r4Player.ws"),
            roots: vec![dir("proj")],
            legacy: vec![],
            replacements: HashMap::new(),
            game: Some(dir("game")),
            additional: vec![],
            expected: FileScope::AdditionalBase,
        },
        Case {
            name: "under an additional script directory",
            uri: url("extra/Lib.ws"),
            roots: vec![dir("proj")],
            legacy: vec![],
            replacements: HashMap::new(),
            game: None,
            additional: vec![dir("extra")],
            expected: FileScope::AdditionalBase,
        },
        Case {
            name: "legacy precedence beats additional for the same dir",
            uri: url("shared/game/r4Player.ws"),
            roots: vec![dir("proj")],
            legacy: vec![dir("shared")],
            replacements: HashMap::new(),
            game: None,
            additional: vec![dir("shared")],
            expected: FileScope::LegacyNew,
        },
        Case {
            name: "workspace root nested over a legacy dir wins",
            uri: url("proj/legacy/game/r4Player.ws"),
            roots: vec![dir("proj")],
            legacy: vec![dir("proj/legacy")],
            replacements: HashMap::new(),
            game: None,
            additional: vec![],
            expected: FileScope::InProject,
        },
    ];

    for c in cases {
        let got = classify_file_scope(
            &c.uri,
            &c.roots,
            &c.legacy,
            &c.replacements,
            c.game.as_deref(),
            &c.additional,
        );
        assert_eq!(got, c.expected, "case '{}': scope mismatch", c.name);
    }
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
