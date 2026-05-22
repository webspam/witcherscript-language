use std::sync::Arc;

use lsp_types::request::WorkspaceConfiguration;
use lsp_types::{ConfigurationItem, ConfigurationParams};
use serde_json::Value;
use tracing::{info, trace, warn};

use crate::backend::Backend;
use crate::logging::{level_from_str, level_to_u8, DEFAULT_LOG_LEVEL};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticsScope {
    Workspace,
    OpenFiles,
}

impl DiagnosticsScope {
    pub(crate) fn from_setting(value: &str) -> Self {
        if value == "openFiles" {
            Self::OpenFiles
        } else {
            Self::Workspace
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    pub(crate) log_level: u8,
    pub(crate) auto_load_mod_shared_imports: bool,
    pub(crate) diagnostics_enabled: bool,
    pub(crate) diagnostics_scope: DiagnosticsScope,
    pub(crate) formatter_line_limit: u32,
    pub(crate) formatter_compact_colon: bool,
    pub(crate) formatter_align_member_colons: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: level_to_u8(DEFAULT_LOG_LEVEL),
            auto_load_mod_shared_imports: true,
            diagnostics_enabled: true,
            diagnostics_scope: DiagnosticsScope::Workspace,
            formatter_line_limit: 100,
            formatter_compact_colon: false,
            formatter_align_member_colons: false,
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
}

impl Backend {
    pub(crate) async fn fetch_config(&self) -> ConfigChange {
        let prev_base_scripts_path = self.base_scripts_path.lock().await.clone();
        let prev_files_exclude = self.files_exclude.lock().await.clone();
        let prev_additional = self.additional_script_dirs.lock().await.clone();
        let prev_legacy = self.legacy_script_dirs.lock().await.clone();
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
                section: Some("witcherscript.diagnostics.enable".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.legacyScriptDirectories".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.diagnostics.scope".to_string()),
            },
        ];
        let Ok(values) = self
            .client
            .request::<WorkspaceConfiguration>(ConfigurationParams { items })
            .await
        else {
            warn!("workspace/configuration request failed");
            return ConfigChange::default();
        };
        let mut iter = values.into_iter();
        let mut next_cfg = prev_cfg.clone();

        if let Some(Value::String(path_str)) = iter.next() {
            if !path_str.is_empty() {
                *self.base_scripts_path.lock().await = Some(std::path::PathBuf::from(path_str));
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
        if let Some(Value::Object(map)) = iter.next() {
            let globs: Vec<String> = map
                .into_iter()
                .filter(|(_, enabled)| matches!(enabled, Value::Bool(true)))
                .map(|(glob, _)| glob)
                .collect();
            *self.files_exclude.lock().await = globs;
        }
        match iter.next() {
            Some(Value::Array(arr)) => {
                let dirs: Vec<std::path::PathBuf> = arr
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::String(s) if !s.is_empty() => Some(std::path::PathBuf::from(s)),
                        _ => None,
                    })
                    .collect();
                *self.additional_script_dirs.lock().await = dirs;
            }
            _ => {
                self.additional_script_dirs.lock().await.clear();
            }
        }
        next_cfg.auto_load_mod_shared_imports = match iter.next() {
            Some(Value::Bool(b)) => b,
            _ => true,
        };
        next_cfg.diagnostics_enabled = match iter.next() {
            Some(Value::Bool(b)) => b,
            _ => true,
        };
        match iter.next() {
            Some(Value::Array(arr)) => {
                let dirs: Vec<std::path::PathBuf> = arr
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::String(s) if !s.is_empty() => Some(std::path::PathBuf::from(s)),
                        _ => None,
                    })
                    .collect();
                *self.legacy_script_dirs.lock().await = dirs;
            }
            _ => {
                self.legacy_script_dirs.lock().await.clear();
            }
        }
        next_cfg.diagnostics_scope = match iter.next() {
            Some(Value::String(s)) => DiagnosticsScope::from_setting(&s),
            _ => DiagnosticsScope::Workspace,
        };

        self.config.store(Arc::new(next_cfg.clone()));

        let base_scripts_changed = *self.base_scripts_path.lock().await != prev_base_scripts_path;
        let files_exclude_changed = *self.files_exclude.lock().await != prev_files_exclude;
        let new_additional_len = self.additional_script_dirs.lock().await.len();
        let additional_changed = new_additional_len != prev_additional.len()
            || *self.additional_script_dirs.lock().await != prev_additional;
        let legacy_changed = *self.legacy_script_dirs.lock().await != prev_legacy;
        let auto_load_changed =
            next_cfg.auto_load_mod_shared_imports != prev_cfg.auto_load_mod_shared_imports;
        let diagnostics_changed = next_cfg.diagnostics_enabled != prev_cfg.diagnostics_enabled
            || next_cfg.diagnostics_scope != prev_cfg.diagnostics_scope;
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
        if diagnostics_changed {
            trace!(setting = "diagnostics", "setting changed");
        }
        if legacy_changed {
            trace!(setting = "legacyScriptDirectories", "setting changed");
        }
        ConfigChange {
            needs_reindex: base_scripts_changed
                || files_exclude_changed
                || additional_changed
                || legacy_changed
                || auto_load_changed,
            diagnostics_changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigChange, DiagnosticsScope};

    #[test]
    fn diagnostics_scope_parses_setting_string() {
        struct Case {
            name: &'static str,
            input: &'static str,
            expected: DiagnosticsScope,
        }
        let cases = [
            Case {
                name: "explicit openFiles",
                input: "openFiles",
                expected: DiagnosticsScope::OpenFiles,
            },
            Case {
                name: "explicit workspace",
                input: "workspace",
                expected: DiagnosticsScope::Workspace,
            },
            Case {
                name: "unknown value falls back to workspace",
                input: "garbage",
                expected: DiagnosticsScope::Workspace,
            },
        ];
        for c in cases {
            assert_eq!(
                DiagnosticsScope::from_setting(c.input),
                c.expected,
                "case {}: scope mismatch",
                c.name
            );
        }
    }

    #[test]
    fn config_change_default_is_no_op() {
        let c = ConfigChange::default();
        struct Case {
            name: &'static str,
            change: ConfigChange,
            expect_any_action: bool,
        }
        let cases = [
            Case {
                name: "default → nothing to do",
                change: c,
                expect_any_action: false,
            },
            Case {
                name: "reindex only",
                change: ConfigChange {
                    needs_reindex: true,
                    diagnostics_changed: false,
                },
                expect_any_action: true,
            },
            Case {
                name: "diagnostics change only",
                change: ConfigChange {
                    needs_reindex: false,
                    diagnostics_changed: true,
                },
                expect_any_action: true,
            },
            Case {
                name: "both at once",
                change: ConfigChange {
                    needs_reindex: true,
                    diagnostics_changed: true,
                },
                expect_any_action: true,
            },
        ];
        for c in cases {
            let any = c.change.needs_reindex || c.change.diagnostics_changed;
            assert_eq!(
                any, c.expect_any_action,
                "case {}: action predicate wrong",
                c.name
            );
        }
    }
}
