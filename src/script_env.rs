use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use lsp_types::Url;

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
            symbol: global_symbol(name, type_name, range, byte_start..byte_end),
        });
    }

    apply_engine_overrides(&mut globals, &ini_uri);

    Some(ScriptEnvironment::new(globals))
}

// Engine injects these globals at runtime; if customised, leave them
fn apply_engine_overrides(globals: &mut Vec<ScriptGlobal>, ini_uri: &str) {
    override_stock_global(globals, ini_uri, "theCamera", "CCamera", "CCameraDirector");
    inject_if_absent(globals, ini_uri, "theTelemetry", "CR4TelemetryScriptProxy");
}

fn override_stock_global(
    globals: &mut Vec<ScriptGlobal>,
    ini_uri: &str,
    name: &str,
    stock_type: &str,
    override_type: &str,
) {
    match globals.iter_mut().find(|g| g.name == name) {
        Some(existing) if existing.type_name == stock_type => {
            existing.type_name = override_type.to_string();
            existing.symbol.type_annotation = Some(override_type.to_string());
        }
        // retyped by the user to a custom class; leave their choice alone
        Some(_) => {}
        None => globals.push(make_global(ini_uri, name, override_type)),
    }
}

fn inject_if_absent(globals: &mut Vec<ScriptGlobal>, ini_uri: &str, name: &str, type_name: &str) {
    if globals.iter().any(|g| g.name == name) {
        return;
    }
    globals.push(make_global(ini_uri, name, type_name));
}

fn make_global(ini_uri: &str, name: &str, type_name: &str) -> ScriptGlobal {
    let zero = SourcePosition {
        line: 0,
        character: 0,
    };
    let range = SourceRange {
        start: zero,
        end: zero,
    };
    ScriptGlobal {
        name: name.to_string(),
        type_name: type_name.to_string(),
        ini_uri: ini_uri.to_string(),
        symbol: global_symbol(name, type_name, range, 0..0),
    }
}

fn global_symbol(
    name: &str,
    type_name: &str,
    range: SourceRange,
    byte_range: std::ops::Range<usize>,
) -> Symbol {
    Symbol {
        id: SymbolId(0),
        name: name.to_string(),
        kind: SymbolKind::Variable,
        range,
        selection_range: range,
        byte_range: byte_range.clone(),
        selection_byte_range: byte_range,
        container: None,
        container_name: None,
        type_annotation: Some(type_name.to_string()),
        signature: None,
        base_class: None,
        owner_class: None,
        flavour: None,
        annotations: Vec::new(),
        access: AccessLevel::Public,
        is_optional: false,
        is_out: false,
    }
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
        assert_eq!(env.find("theGame").unwrap().type_name, "CR4Game");
        assert_eq!(env.find("thePlayer").unwrap().type_name, "CR4Player");
    }

    #[test]
    fn skips_other_sections_and_comments() {
        let path = write_temp(
            "se_test2.ini",
            "[other]\nfoo=Bar\n[globals]\n; skip\ntheGame=CR4Game\n[more]\nbaz=Qux\n",
        );
        let env = parse_script_environment(&path).unwrap();
        assert!(env.find("theGame").is_some());
        assert!(env.find("foo").is_none());
        assert!(env.find("baz").is_none());
    }

    #[test]
    fn symbol_has_correct_position() {
        let path = write_temp("se_test3.ini", "[globals]\ntheGame=CR4Game\n");
        let env = parse_script_environment(&path).unwrap();
        let sym = &env.find("theGame").unwrap().symbol;
        assert_eq!(sym.selection_range.start.line, 1);
        assert_eq!(sym.selection_range.start.character, 0);
        assert_eq!(sym.type_annotation.as_deref(), Some("CR4Game"));
        assert_eq!(sym.kind, SymbolKind::Variable);
    }

    #[test]
    fn camera_injected_when_absent_from_ini() {
        let path = write_temp("se_camera1.ini", "[globals]\ntheGame=CR4Game\n");
        let env = parse_script_environment(&path).unwrap();
        let camera = env.find("theCamera").expect("theCamera injected");
        assert_eq!(camera.type_name, "CCameraDirector");
        assert_eq!(
            camera.symbol.type_annotation.as_deref(),
            Some("CCameraDirector")
        );
    }

    #[test]
    fn camera_override_respects_ini_state() {
        struct Case {
            name: &'static str,
            ini_type: &'static str,
            expected: &'static str,
        }
        let cases = [
            Case {
                name: "stock CCamera entry is overridden",
                ini_type: "CCamera",
                expected: "CCameraDirector",
            },
            Case {
                name: "mod retyped is left untouched",
                ini_type: "MyCustomCamera",
                expected: "MyCustomCamera",
            },
        ];
        for (idx, c) in cases.iter().enumerate() {
            let path = write_temp(
                &format!("se_camera_state{idx}.ini"),
                &format!("[globals]\ntheCamera={}\n", c.ini_type),
            );
            let env = parse_script_environment(&path).unwrap();
            let camera = env.find("theCamera").unwrap();
            assert_eq!(camera.type_name, c.expected, "case: {}", c.name);
            assert_eq!(
                camera.symbol.type_annotation.as_deref(),
                Some(c.expected),
                "case: {}",
                c.name,
            );
        }
    }

    #[test]
    fn telemetry_is_appended_even_without_ini_entry() {
        let path = write_temp("se_telemetry1.ini", "[globals]\ntheGame=CR4Game\n");
        let env = parse_script_environment(&path).unwrap();
        let tel = env.find("theTelemetry").expect("theTelemetry injected");
        assert_eq!(tel.type_name, "CR4TelemetryScriptProxy");
        assert_eq!(tel.symbol.kind, SymbolKind::Variable);
    }

    #[test]
    fn telemetry_ini_entry_is_not_overwritten() {
        let path = write_temp(
            "se_telemetry2.ini",
            "[globals]\ntheTelemetry=SomeOtherTelemetry\n",
        );
        let env = parse_script_environment(&path).unwrap();
        let matches: Vec<_> = env
            .globals
            .iter()
            .filter(|g| g.name == "theTelemetry")
            .collect();
        assert_eq!(matches.len(), 1, "should not duplicate theTelemetry");
        assert_eq!(matches[0].type_name, "SomeOtherTelemetry");
    }
}
