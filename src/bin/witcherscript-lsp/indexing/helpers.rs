use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_types::Url;
use tracing::warn;
use witcherscript_language::diagnostics::{basename_of, relative_from_scripts};
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::{ObservedKey, WorkspaceIndex};

pub(super) fn path_to_canonical_uri(path: &Path) -> Option<String> {
    Url::from_file_path(path).ok().map(|u| canonical_uri(&u))
}

pub(crate) fn legacy_replaces_base(base_uri: &str, legacy_uri: &str) -> bool {
    let Some(tail) = relative_from_scripts(base_uri) else {
        return false;
    };
    let needle = format!("/{tail}");
    legacy_uri.ends_with(&needle)
}

pub(crate) fn legacy_base_replacements(
    base_uris: &[String],
    legacy_uris: &[String],
) -> (HashSet<String>, HashMap<String, String>) {
    let mut base_by_basename: HashMap<&str, Vec<&String>> = HashMap::new();
    for base_uri in base_uris {
        if let Some(name) = basename_of(base_uri) {
            base_by_basename.entry(name).or_default().push(base_uri);
        }
    }
    let mut skip_base: HashSet<String> = HashSet::new();
    let mut replacements: HashMap<String, String> = HashMap::new();
    for legacy_uri in legacy_uris {
        let Some(candidates) = basename_of(legacy_uri).and_then(|name| base_by_basename.get(name))
        else {
            continue;
        };
        let canonical = Url::parse(legacy_uri)
            .ok().map_or_else(|| legacy_uri.clone(), |u| canonical_uri(&u));
        for base_uri in candidates {
            if legacy_replaces_base(base_uri, legacy_uri) {
                skip_base.insert((*base_uri).clone());
                if let Some(rel) = relative_from_scripts(base_uri) {
                    replacements.insert(canonical.clone(), rel.to_string());
                }
            }
        }
    }
    (skip_base, replacements)
}

pub(crate) fn build_index_segments(
    base_scripts_dir: Option<&Path>,
    extras: &[PathBuf],
) -> Vec<(&'static str, PathBuf)> {
    let mut segments: Vec<(&'static str, PathBuf)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());

    if let Some(base) = base_scripts_dir
        && seen.insert(canon(base))
    {
        segments.push(("gameDirectory", base.to_path_buf()));
    }

    for extra in extras {
        if !extra.is_dir() {
            warn!(path = %extra.display(), "additionalScriptDirectories entry is not a directory; skipping");
            continue;
        }
        if seen.insert(canon(extra)) {
            segments.push(("additionalScriptDirectory", extra.clone()));
        }
    }

    segments
}

// modSharedImports ships replacement scripts, so it is indexed as a legacy
// script dir rather than a base overlay.
pub(crate) fn mod_shared_imports_dir(game_dir: &Path) -> Option<PathBuf> {
    let msi = game_dir.join(r"Mods\modSharedImports");
    msi.is_dir().then_some(msi)
}

pub(crate) fn index_open_document(
    index: &mut WorkspaceIndex,
    uri: &Url,
    document: &ParsedDocument,
) -> Vec<ObservedKey> {
    let canonical = canonical_uri(uri);
    let mut changed = Vec::new();
    if canonical != uri.as_str() {
        changed.extend(index.remove_document(uri.as_str()));
    }
    changed.extend(index.update_document(&canonical, document));
    changed
}

pub(crate) fn remove_document_all_spellings(
    index: &mut WorkspaceIndex,
    uri: &Url,
) -> Vec<ObservedKey> {
    let mut changed = index.remove_document(uri.as_str());
    let canonical = canonical_uri(uri);
    if canonical != uri.as_str() {
        changed.extend(index.remove_document(&canonical));
    }
    changed
}

// A closed file reverts to disk content, re-keyed from the open spelling to canonical.
pub(super) fn reindex_into(
    index: &mut WorkspaceIndex,
    docs: &mut HashMap<String, Arc<ParsedDocument>>,
    client_uri: &str,
    canonical: &str,
    parsed: Option<ParsedDocument>,
) -> Vec<ObservedKey> {
    let mut changed = Vec::new();
    if client_uri != canonical {
        changed.extend(index.remove_document(client_uri));
    }
    match parsed {
        Some(document) => {
            changed.extend(index.update_document(canonical, &document));
            docs.insert(canonical.to_string(), Arc::new(document));
        }
        None => {
            changed.extend(index.remove_document(canonical));
            docs.remove(canonical);
        }
    }
    changed
}
