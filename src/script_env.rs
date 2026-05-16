use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use tower_lsp::lsp_types::Url;

use crate::line_index::{SourcePosition, SourceRange};
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

static SCRIPT_ENV_VERSION: AtomicU64 = AtomicU64::new(0);

fn next_script_env_version() -> u64 {
    SCRIPT_ENV_VERSION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug)]
pub struct ScriptGlobal {
    pub name: String,
    pub type_name: String,
    pub ini_uri: String,
    pub symbol: Symbol,
}

#[derive(Debug)]
pub struct ScriptEnvironment {
    pub globals: Vec<ScriptGlobal>,
    version: u64,
}

impl Default for ScriptEnvironment {
    fn default() -> Self {
        Self {
            globals: Vec::new(),
            version: next_script_env_version(),
        }
    }
}

impl ScriptEnvironment {
    pub fn new(globals: Vec<ScriptGlobal>) -> Self {
        Self {
            globals,
            version: next_script_env_version(),
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn find(&self, name: &str) -> Option<&ScriptGlobal> {
        self.globals.iter().find(|g| g.name == name)
    }
}

pub fn parse_script_environment(ini_path: &Path) -> Option<ScriptEnvironment> {
    let content = std::fs::read_to_string(ini_path).ok()?;
    let ini_uri = Url::from_file_path(ini_path).ok()?.to_string();

    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(
            content
                .bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        )
        .collect();

    let mut in_globals = false;
    let mut globals = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_globals = trimmed.eq_ignore_ascii_case("[globals]");
            continue;
        }
        if !in_globals || trimmed.starts_with(';') || trimmed.is_empty() {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else {
            continue;
        };
        let name = key.trim();
        let type_name = val.trim();
        if name.is_empty() || type_name.is_empty() {
            continue;
        }

        let key_col = line.find(name).unwrap_or(0);
        let line_start = line_starts[line_idx];
        let byte_start = line_start + key_col;
        let byte_end = byte_start + name.len();

        let start = SourcePosition {
            line: line_idx as u32,
            character: key_col as u32,
        };
        let end = SourcePosition {
            line: line_idx as u32,
            character: (key_col + name.len()) as u32,
        };
        let range = SourceRange { start, end };

        globals.push(ScriptGlobal {
            name: name.to_string(),
            type_name: type_name.to_string(),
            ini_uri: ini_uri.clone(),
            symbol: Symbol {
                id: SymbolId(0),
                name: name.to_string(),
                kind: SymbolKind::Variable,
                range,
                selection_range: range,
                byte_range: byte_start..byte_end,
                selection_byte_range: byte_start..byte_end,
                container: None,
                container_name: None,
                type_annotation: Some(type_name.to_string()),
                signature: None,
                detail: None,
                base_class: None,
                owner_class: None,
                flavour: None,
                annotations: Vec::new(),
                access: AccessLevel::Public,
                is_optional: false,
                is_out: false,
            },
        });
    }

    Some(ScriptEnvironment::new(globals))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parses_globals_section() {
        let path = write_temp(
            "se_test1.ini",
            "[globals]\ntheGame=CR4Game\nthePlayer=CR4Player\n",
        );
        let env = parse_script_environment(&path).unwrap();
        assert_eq!(env.globals.len(), 2);
        assert_eq!(env.globals[0].name, "theGame");
        assert_eq!(env.globals[0].type_name, "CR4Game");
    }

    #[test]
    fn skips_other_sections_and_comments() {
        let path = write_temp(
            "se_test2.ini",
            "[other]\nfoo=Bar\n[globals]\n; skip\ntheGame=CR4Game\n[more]\nbaz=Qux\n",
        );
        let env = parse_script_environment(&path).unwrap();
        assert_eq!(env.globals.len(), 1);
        assert_eq!(env.globals[0].name, "theGame");
    }

    #[test]
    fn symbol_has_correct_position() {
        let path = write_temp("se_test3.ini", "[globals]\ntheGame=CR4Game\n");
        let env = parse_script_environment(&path).unwrap();
        let sym = &env.globals[0].symbol;
        assert_eq!(sym.selection_range.start.line, 1);
        assert_eq!(sym.selection_range.start.character, 0);
        assert_eq!(sym.type_annotation.as_deref(), Some("CR4Game"));
        assert_eq!(sym.kind, SymbolKind::Variable);
    }
}
