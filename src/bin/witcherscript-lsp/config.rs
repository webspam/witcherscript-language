use std::sync::Arc;

use lsp_types::request::WorkspaceConfiguration;
use lsp_types::{ConfigurationItem, ConfigurationParams};
use serde_json::Value;
use tracing::{info, trace, warn};

use crate::backend::Backend;
use crate::logging::{level_from_str, level_to_u8, DEFAULT_LOG_LEVEL};
use witcherscript_language::formatter::AnnotationPlacement;

fn parse_path_array(value: Option<Value>) -> Vec<std::path::PathBuf> {
    let Some(Value::Array(arr)) = value else {
        return Vec::new();
    };
    arr.into_iter()
        .filter_map(|v| match v {
            Value::String(s) if !s.is_empty() => Some(std::path::PathBuf::from(s)),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticsScope {
    Workspace,
    OpenFiles,
    None,
}

impl DiagnosticsScope {
    pub(crate) fn from_setting(value: &str) -> Self {
        match value {
            "openFiles" => Self::OpenFiles,
            "none" => Self::None,
            _ => Self::Workspace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    pub(crate) log_level: u8,
    pub(crate) auto_load_mod_shared_imports: bool,
    pub(crate) auto_detect_project_manifests: bool,
    pub(crate) diagnostics_scope: DiagnosticsScope,
    pub(crate) formatter_line_limit: u32,
    pub(crate) formatter_compact_colon: bool,
    pub(crate) formatter_align_member_colons: bool,
    pub(crate) formatter_annotation_placement: AnnotationPlacement,
    pub(crate) formatter_default_placement: AnnotationPlacement,
    pub(crate) code_lens_overridden_symbols: bool,
    pub(crate) code_lens_references: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: level_to_u8(DEFAULT_LOG_LEVEL),
            auto_load_mod_shared_imports: true,
            auto_detect_project_manifests: true,
            diagnostics_scope: DiagnosticsScope::Workspace,
            formatter_line_limit: 100,
            formatter_compact_colon: false,
            formatter_align_member_colons: false,
            formatter_annotation_placement: AnnotationPlacement::Preserve,
            formatter_default_placement: AnnotationPlacement::Preserve,
            code_lens_overridden_symbols: true,
            code_lens_references: false,
        }
    }
}

fn log_setting_change<T: PartialEq + std::fmt::Display>(setting: &str, prev: T, new: T) {
    if prev != new {
        trace!(setting, prev = %prev, new = %new, "setting changed");
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub(crate) struct ConfigChange {
    pub(crate) needs_reindex: bool,
    pub(crate) diagnostics_changed: bool,
    pub(crate) code_lens_changed: bool,
}

impl Backend {
    pub(crate) async fn fetch_config(&self) -> ConfigChange {
        let started_at = std::time::Instant::now();
        tracing::debug!(op = "fetch_config", "start",);
        let prev_base_scripts_path = self.game_directory.lock().clone();
        let prev_files_exclude = self.files_exclude.lock().clone();
        let prev_additional = self.additional_script_dirs.lock().clone();
        let prev_legacy = self.legacy_script_dirs.lock().clone();
        let prev_cfg = (**self.config.load()).clone();

        let items = vec![
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.gameDirectory".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.logLevel".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.lineLimit".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.compactColon".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.alignMemberColons".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.annotationPlacement".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.defaultPlacement".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("files.exclude".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.additionalScriptDirectories".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.autoLoadModSharedImports".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.legacyScriptDirectories".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.detectProjectManifests".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.diagnostics.scope".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.codeLens.overriddenSymbols".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.codeLens.references".to_string()),
            },
        ];
        let Ok(values) = self
            .client
            .request::<WorkspaceConfiguration>(ConfigurationParams { items })
            .await
        else {
            warn!("workspace/configuration request failed");
            tracing::debug!(
                op = "fetch_config",
                elapsed_us = started_at.elapsed().as_micros(),
                reason = "request_failed",
                "complete",
            );
            return ConfigChange::default();
        };
        let mut iter = values.into_iter();
        let mut next_cfg = prev_cfg.clone();

        if let Some(Value::String(path_str)) = iter.next() {
            if !path_str.is_empty() {
                *self.game_directory.lock() = Some(std::path::PathBuf::from(path_str));
            }
        }
        if let Some(Value::String(level_str)) = iter.next() {
            next_cfg.log_level = level_to_u8(level_from_str(&level_str));
            if next_cfg.log_level != prev_cfg.log_level {
                info!(level = %level_str, "log level updated");
            }
        }
        if let Some(Value::Number(n)) = iter.next() {
            if let Some(limit) = n.as_u64() {
                next_cfg.formatter_line_limit = limit as u32;
                log_setting_change(
                    "formatter.lineLimit",
                    prev_cfg.formatter_line_limit,
                    next_cfg.formatter_line_limit,
                );
            }
        }
        if let Some(Value::Bool(compact)) = iter.next() {
            next_cfg.formatter_compact_colon = compact;
            log_setting_change(
                "formatter.compactColon",
                prev_cfg.formatter_compact_colon,
                next_cfg.formatter_compact_colon,
            );
        }
        if let Some(Value::Bool(align)) = iter.next() {
            next_cfg.formatter_align_member_colons = align;
            log_setting_change(
                "formatter.alignMemberColons",
                prev_cfg.formatter_align_member_colons,
                next_cfg.formatter_align_member_colons,
            );
        }
        if let Some(Value::String(placement)) = iter.next() {
            next_cfg.formatter_annotation_placement = AnnotationPlacement::from_setting(&placement);
            log_setting_change(
                "formatter.annotationPlacement",
                prev_cfg.formatter_annotation_placement,
                next_cfg.formatter_annotation_placement,
            );
        }
        if let Some(Value::String(placement)) = iter.next() {
            next_cfg.formatter_default_placement = AnnotationPlacement::from_setting(&placement);
            log_setting_change(
                "formatter.defaultPlacement",
                prev_cfg.formatter_default_placement,
                next_cfg.formatter_default_placement,
            );
        }
        if let Some(Value::Object(map)) = iter.next() {
            let globs: Vec<String> = map
                .into_iter()
                .filter(|(_, enabled)| matches!(enabled, Value::Bool(true)))
                .map(|(glob, _)| glob)
                .collect();
            *self.files_exclude.lock() = globs;
        }
        *self.additional_script_dirs.lock() = parse_path_array(iter.next());
        next_cfg.auto_load_mod_shared_imports = match iter.next() {
            Some(Value::Bool(b)) => b,
            _ => true,
        };
        *self.legacy_script_dirs.lock() = parse_path_array(iter.next());
        next_cfg.auto_detect_project_manifests = match iter.next() {
            Some(Value::Bool(b)) => b,
            _ => true,
        };
        next_cfg.diagnostics_scope = match iter.next() {
            Some(Value::String(s)) => DiagnosticsScope::from_setting(&s),
            _ => DiagnosticsScope::Workspace,
        };
        next_cfg.code_lens_overridden_symbols = match iter.next() {
            Some(Value::Bool(b)) => b,
            _ => true,
        };
        next_cfg.code_lens_references = match iter.next() {
            Some(Value::Bool(b)) => b,
            _ => false,
        };

        self.config.store(Arc::new(next_cfg.clone()));

        let base_scripts_changed = *self.game_directory.lock() != prev_base_scripts_path;
        let files_exclude_changed = *self.files_exclude.lock() != prev_files_exclude;
        let new_additional_len = self.additional_script_dirs.lock().len();
        let additional_changed = new_additional_len != prev_additional.len()
            || *self.additional_script_dirs.lock() != prev_additional;
        let legacy_changed = *self.legacy_script_dirs.lock() != prev_legacy;
        let auto_load_changed =
            next_cfg.auto_load_mod_shared_imports != prev_cfg.auto_load_mod_shared_imports;
        let auto_detect_manifests_changed =
            next_cfg.auto_detect_project_manifests != prev_cfg.auto_detect_project_manifests;
        let diagnostics_changed = next_cfg.diagnostics_scope != prev_cfg.diagnostics_scope;
        let code_lens_changed = next_cfg.code_lens_overridden_symbols
            != prev_cfg.code_lens_overridden_symbols
            || next_cfg.code_lens_references != prev_cfg.code_lens_references;
        if base_scripts_changed {
            trace!(setting = "gameDirectory", "setting changed");
        }
        if files_exclude_changed {
            trace!(setting = "files.exclude", "setting changed");
        }
        if additional_changed {
            trace!(
                setting = "additionalScriptDirectories",
                prev = prev_additional.len(),
                new = new_additional_len,
                "setting changed"
            );
        }
        if auto_load_changed {
            trace!(
                setting = "autoLoadModSharedImports",
                prev = prev_cfg.auto_load_mod_shared_imports,
                new = next_cfg.auto_load_mod_shared_imports,
                "setting changed"
            );
        }
        if auto_detect_manifests_changed {
            trace!(
                setting = "detectProjectManifests",
                prev = prev_cfg.auto_detect_project_manifests,
                new = next_cfg.auto_detect_project_manifests,
                "setting changed"
            );
        }
        if diagnostics_changed {
            trace!(setting = "diagnostics", "setting changed");
        }
        if next_cfg.code_lens_overridden_symbols != prev_cfg.code_lens_overridden_symbols {
            trace!(
                setting = "codeLens.overriddenSymbols",
                prev = prev_cfg.code_lens_overridden_symbols,
                new = next_cfg.code_lens_overridden_symbols,
                "setting changed"
            );
        }
        if next_cfg.code_lens_references != prev_cfg.code_lens_references {
            trace!(
                setting = "codeLens.references",
                prev = prev_cfg.code_lens_references,
                new = next_cfg.code_lens_references,
                "setting changed"
            );
        }
        if legacy_changed {
            trace!(setting = "legacyScriptDirectories", "setting changed");
        }
        let change = ConfigChange {
            needs_reindex: base_scripts_changed
                || files_exclude_changed
                || additional_changed
                || legacy_changed
                || auto_load_changed
                || auto_detect_manifests_changed,
            diagnostics_changed,
            code_lens_changed,
        };
        tracing::debug!(
            op = "fetch_config",
            elapsed_us = started_at.elapsed().as_micros(),
            needs_reindex = change.needs_reindex,
            diagnostics_changed = change.diagnostics_changed,
            "complete",
        );
        change
    }
}

#[cfg(test)]
mod tests;
