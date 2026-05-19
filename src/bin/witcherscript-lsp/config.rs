use std::sync::atomic::Ordering;

use lsp_types::request::WorkspaceConfiguration;
use lsp_types::{ConfigurationItem, ConfigurationParams};
use serde_json::Value;
use tracing::{info, trace, warn};

use crate::backend::Backend;
use crate::logging::{level_from_str, level_to_u8};

fn log_setting_change<T: PartialEq + std::fmt::Display>(setting: &str, prev: T, new: T) {
    if prev != new {
        trace!(setting, prev = %prev, new = %new, "setting changed");
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub(crate) struct ConfigChange {
    pub(crate) needs_reindex: bool,
    pub(crate) diagnostics_toggled: bool,
}

impl Backend {
    pub(crate) async fn fetch_config(&self) -> ConfigChange {
        let prev_base_scripts_path = self.base_scripts_path.lock().await.clone();
        let prev_files_exclude = self.files_exclude.lock().await.clone();
        let prev_additional = self.additional_script_dirs.lock().await.clone();
        let prev_auto_load = self.auto_load_mod_shared_imports.load(Ordering::Relaxed);
        let prev_diag_enabled = self.diagnostics_enabled.load(Ordering::Relaxed);

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
        if let Some(Value::String(path_str)) = iter.next() {
            if !path_str.is_empty() {
                *self.base_scripts_path.lock().await = Some(std::path::PathBuf::from(path_str));
            }
        }
        if let Some(Value::String(level_str)) = iter.next() {
            let new_level = level_to_u8(level_from_str(&level_str));
            self.log_level.store(new_level, Ordering::Relaxed);
            info!(level = %level_str, "log level updated");
        }
        if let Some(Value::Number(n)) = iter.next() {
            if let Some(limit) = n.as_u64() {
                log_setting_change(
                    "formatter.lineLimit",
                    self.formatter_line_limit
                        .swap(limit as u32, Ordering::Relaxed),
                    limit as u32,
                );
            }
        }
        if let Some(Value::Bool(compact)) = iter.next() {
            log_setting_change(
                "formatter.compactColon",
                self.formatter_compact_colon
                    .swap(compact, Ordering::Relaxed),
                compact,
            );
        }
        if let Some(Value::Bool(align)) = iter.next() {
            log_setting_change(
                "formatter.alignMemberColons",
                self.formatter_align_member_colons
                    .swap(align, Ordering::Relaxed),
                align,
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
        match iter.next() {
            Some(Value::Bool(b)) => {
                self.auto_load_mod_shared_imports
                    .store(b, Ordering::Relaxed);
            }
            _ => {
                self.auto_load_mod_shared_imports
                    .store(true, Ordering::Relaxed);
            }
        }
        match iter.next() {
            Some(Value::Bool(b)) => {
                self.diagnostics_enabled.store(b, Ordering::Relaxed);
            }
            _ => {
                self.diagnostics_enabled.store(true, Ordering::Relaxed);
            }
        }

        let base_scripts_changed = *self.base_scripts_path.lock().await != prev_base_scripts_path;
        let files_exclude_changed = *self.files_exclude.lock().await != prev_files_exclude;
        let new_additional_len = self.additional_script_dirs.lock().await.len();
        let additional_changed = new_additional_len != prev_additional.len()
            || *self.additional_script_dirs.lock().await != prev_additional;
        let new_auto_load = self.auto_load_mod_shared_imports.load(Ordering::Relaxed);
        let auto_load_changed = new_auto_load != prev_auto_load;
        let new_diag_enabled = self.diagnostics_enabled.load(Ordering::Relaxed);
        let diagnostics_toggled = new_diag_enabled != prev_diag_enabled;
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
                prev = prev_auto_load,
                new = new_auto_load,
                "setting changed"
            );
        }
        if diagnostics_toggled {
            trace!(
                setting = "diagnostics.enable",
                prev = prev_diag_enabled,
                new = new_diag_enabled,
                "setting changed"
            );
        }
        ConfigChange {
            needs_reindex: base_scripts_changed
                || files_exclude_changed
                || additional_changed
                || auto_load_changed,
            diagnostics_toggled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigChange;

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
                    diagnostics_toggled: false,
                },
                expect_any_action: true,
            },
            Case {
                name: "diagnostics toggle only",
                change: ConfigChange {
                    needs_reindex: false,
                    diagnostics_toggled: true,
                },
                expect_any_action: true,
            },
            Case {
                name: "both at once",
                change: ConfigChange {
                    needs_reindex: true,
                    diagnostics_toggled: true,
                },
                expect_any_action: true,
            },
        ];
        for c in cases {
            let any = c.change.needs_reindex || c.change.diagnostics_toggled;
            assert_eq!(
                any, c.expect_any_action,
                "case {}: action predicate wrong",
                c.name
            );
        }
    }
}
