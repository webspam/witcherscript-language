use std::collections::HashMap;
use std::path::PathBuf;

use crate::resolve::WorkspaceIndex;
use crate::symbols::{Symbol, SymbolKind};

use super::{RelatedLocation, Severity, WorkspaceDiagnostic};

const SCRIPTS_SEGMENT: &str = "/scripts/";
pub const KIND: &str = "base_script_conflict";

struct BaseEntry<'a> {
    uri: &'a str,
    relative_path: &'a str,
    declarations: HashMap<(&'a str, Option<&'a str>), &'a Symbol>,
}

pub fn collect_base_script_conflict_diagnostics(
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
    legacy_dirs: &[PathBuf],
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let by_basename = index_base_scripts(base);
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (workspace_uri, symbols) in workspace.documents() {
        // A file under a legacyScriptDirectories entry is a confirmed override, not a clash.
        if uri_is_under_any(workspace_uri, legacy_dirs) {
            continue;
        }
        let Some(basename) = basename_of(workspace_uri) else {
            continue;
        };
        let Some(entries) = by_basename.get(basename) else {
            continue;
        };

        for sym in symbols {
            if !is_eligible(sym) {
                continue;
            }
            let key = (sym.name.as_str(), owner_for_state(sym));

            let Some((entry, base_sym)) = entries.iter().find_map(|entry| {
                if is_same_file(workspace_uri, entry.uri) {
                    return None;
                }
                if !path_suffix_matches(workspace_uri, entry.relative_path) {
                    return None;
                }
                entry.declarations.get(&key).map(|base| (entry, *base))
            }) else {
                continue;
            };

            let message = format!(
                "'{name}' is already declared in base script '{basename}'. This file looks \
                 like a legacy full-script override. If that is intended, add its directory \
                 to `witcherscript.legacyScriptDirectories`; otherwise use annotation-based \
                 modding (@addMethod / @wrapMethod / @replaceMethod).",
                name = sym.name,
            );

            result
                .entry(workspace_uri.to_string())
                .or_default()
                .push(WorkspaceDiagnostic {
                    kind: KIND.to_string(),
                    message,
                    severity: Severity::Error,
                    range: sym.selection_range,
                    related: vec![RelatedLocation {
                        uri: entry.uri.to_string(),
                        range: base_sym.selection_range,
                        message: format!("base script '{basename}' declares '{}' here", sym.name),
                    }],
                    data: conflict_data(workspace_uri, entry.relative_path),
                });
        }
    }

    for diags in result.values_mut() {
        diags.sort_by_key(|d| (d.range.start.line, d.range.start.character));
    }
    result
}

fn index_base_scripts(base: &WorkspaceIndex) -> HashMap<&str, Vec<BaseEntry<'_>>> {
    let mut map: HashMap<&str, Vec<BaseEntry<'_>>> = HashMap::new();
    for (uri, symbols) in base.documents() {
        let Some(basename) = basename_of(uri) else {
            continue;
        };
        let Some(relative_path) = relative_from_scripts(uri) else {
            continue;
        };
        let mut declarations: HashMap<(&str, Option<&str>), &Symbol> = HashMap::new();
        for sym in symbols {
            if !is_eligible(sym) {
                continue;
            }
            let key = (sym.name.as_str(), owner_for_state(sym));
            declarations.insert(key, sym);
        }
        map.entry(basename).or_default().push(BaseEntry {
            uri,
            relative_path,
            declarations,
        });
    }
    map
}

fn is_eligible(sym: &Symbol) -> bool {
    sym.container.is_none() && is_declaration_kind(sym.kind) && sym.annotations.is_empty()
}

fn is_declaration_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::State
            | SymbolKind::Function
            | SymbolKind::Event
    )
}

fn owner_for_state(sym: &Symbol) -> Option<&str> {
    if sym.kind == SymbolKind::State {
        sym.owner_class.as_deref()
    } else {
        None
    }
}

pub fn basename_of(uri: &str) -> Option<&str> {
    uri.rsplit('/').next().filter(|s| !s.is_empty())
}

pub fn relative_from_scripts(uri: &str) -> Option<&str> {
    let idx = uri.rfind(SCRIPTS_SEGMENT)?;
    Some(&uri[idx + SCRIPTS_SEGMENT.len()..])
}

// Anchor on a separator so `.../r4game/r4Player.ws` does not match base `game/r4Player.ws`.
fn path_suffix_matches(uri: &str, relative_path: &str) -> bool {
    uri.strip_suffix(relative_path)
        .is_some_and(|head| head.ends_with('/'))
}

fn uri_is_under_any(uri: &str, dirs: &[PathBuf]) -> bool {
    if dirs.is_empty() {
        return false;
    }
    let Some(path) = lsp_types::Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
    else {
        return false;
    };
    dirs.iter().any(|dir| path.starts_with(dir))
}

// Carried on the diagnostic so the quick fix knows which directory to mark as legacy.
fn conflict_data(workspace_uri: &str, base_relpath: &str) -> Option<serde_json::Value> {
    let dir_uri = workspace_uri.strip_suffix(base_relpath)?;
    let dir_uri = dir_uri.strip_suffix('/').unwrap_or(dir_uri);
    let path = lsp_types::Url::parse(dir_uri).ok()?.to_file_path().ok()?;
    Some(serde_json::json!({ "directory": path.to_string_lossy() }))
}

// An open base script is keyed by the editor's URI spelling, not the canonical one.
fn is_same_file(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    matches!((canonical_uri(a), canonical_uri(b)), (Some(x), Some(y)) if x == y)
}

fn canonical_uri(uri: &str) -> Option<String> {
    let url = lsp_types::Url::parse(uri).ok()?;
    let path = url.to_file_path().ok()?;
    lsp_types::Url::from_file_path(path)
        .ok()
        .map(|u| u.to_string())
}

#[cfg(test)]
mod tests {
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
                    let diags = result.get(uri).unwrap_or_else(|| {
                        panic!("case '{}': expected diagnostic on {}", c.name, uri)
                    });
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
}
