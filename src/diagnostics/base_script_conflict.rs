use std::collections::HashMap;
use std::path::PathBuf;

use crate::files::canonical_uri;
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
                        message: format!("Base script '{basename}' declares '{}' here", sym.name),
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
    matches!((canonical_uri_from_string(a), canonical_uri_from_string(b)), (Some(x), Some(y)) if x == y)
}

fn canonical_uri_from_string(uri: &str) -> Option<String> {
    let url = lsp_types::Url::parse(uri).ok()?;
    canonical_uri(&url)
}

#[cfg(test)]
mod tests;
