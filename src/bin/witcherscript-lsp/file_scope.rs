use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lsp_types::Url;
use witcherscript_language::files::canonical_uri;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileScope {
    InProject,
    LegacyOverride,
    LegacyNew,
    AdditionalBase,
    OutOfScope,
    SingleFile,
}

impl FileScope {
    pub(crate) fn is_loose(self) -> bool {
        matches!(self, FileScope::OutOfScope | FileScope::SingleFile)
    }
}

pub(crate) fn classify_file_scope(
    uri: &Url,
    workspace_roots: &[PathBuf],
    legacy_script_dirs: &[PathBuf],
    legacy_replacements: &HashMap<String, String>,
    base_scripts_dir: Option<&Path>,
    additional_script_dirs: &[PathBuf],
) -> FileScope {
    let loose = || {
        if workspace_roots.is_empty() {
            FileScope::SingleFile
        } else {
            FileScope::OutOfScope
        }
    };
    let Ok(path) = uri.to_file_path() else {
        return loose();
    };
    if workspace_roots.iter().any(|root| path.starts_with(root)) {
        return FileScope::InProject;
    }
    // Legacy dirs are checked before game/additional dirs, matching is_base_script_uri.
    if legacy_script_dirs.iter().any(|dir| path.starts_with(dir)) {
        let is_override = legacy_replacements.contains_key(&canonical_uri(uri));
        return if is_override {
            FileScope::LegacyOverride
        } else {
            FileScope::LegacyNew
        };
    }
    if base_scripts_dir.is_some_and(|dir| path.starts_with(dir)) {
        return FileScope::AdditionalBase;
    }
    if additional_script_dirs
        .iter()
        .any(|dir| path.starts_with(dir))
    {
        return FileScope::AdditionalBase;
    }
    loose()
}
