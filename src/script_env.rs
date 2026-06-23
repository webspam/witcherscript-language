use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use lsp_types::Url;
use tracing::debug;

use crate::files::read_text_file;
use crate::line_index::{SourcePosition, SourceRange};
use crate::symbols::{AccessLevel, Specifiers, Symbol, SymbolId, SymbolKind};
use crate::types::Type;

static SCRIPT_ENV_VERSION: AtomicU64 = AtomicU64::new(0);

fn next_script_env_version() -> u64 {
    SCRIPT_ENV_VERSION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub struct ScriptGlobal {
    pub name: String,
    pub type_name: String,
    pub ini_uri: String,
    pub symbol: Symbol,
}

#[derive(Debug, Clone)]
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

// theCamera's CCamera is later upgraded to CCameraDirector by apply_engine_overrides.
const FALLBACK_GLOBALS: &[(&str, &str)] = &[
    ("theGame", "CR4Game"),
    ("theServer", "CServerInterface"),
    ("thePlayer", "CR4Player"),
    ("theCamera", "CCamera"),
    ("theUI", "CGuiWitcher"),
    ("theSound", "CScriptSoundSystem"),
    ("theDebug", "CDebugAttributesManager"),
    ("theTimer", "CTimerScriptKeyword"),
    ("theInput", "CInputManager"),
];

pub fn parse_script_environment(ini_path: &Path) -> ScriptEnvironment {
    let ini_uri = Url::from_file_path(ini_path)
        .map(|u| u.to_string())
        .unwrap_or_default();

    let mut globals = match read_text_file(ini_path) {
        Ok(content) => parse_globals(&content, &ini_uri),
        Err(err) => {
            debug!(path = %ini_path.display(), %err, "redscripts.ini unreadable; using fallback globals");
            fallback_globals(&ini_uri)
        }
    };

    apply_engine_overrides(&mut globals, &ini_uri);

    ScriptEnvironment::new(globals)
}

fn fallback_globals(ini_uri: &str) -> Vec<ScriptGlobal> {
    FALLBACK_GLOBALS
        .iter()
        .map(|(name, type_name)| make_global(ini_uri, name, type_name))
        .collect()
}

fn parse_globals(content: &str, ini_uri: &str) -> Vec<ScriptGlobal> {
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

        let start = SourcePosition::new(line_idx, key_col);
        let end = SourcePosition::new(line_idx, key_col + name.len());
        let range = SourceRange { start, end };

        globals.push(ScriptGlobal {
            name: name.to_string(),
            type_name: type_name.to_string(),
            ini_uri: ini_uri.to_string(),
            symbol: global_symbol(name, type_name, range, byte_start..byte_end),
        });
    }

    globals
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
            existing.symbol.type_annotation = Some(Type::from_annotation(override_type));
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
        type_annotation: Some(Type::from_annotation(type_name)),
        base_class: None,
        owner_class: None,
        flavour: None,
        annotations: Vec::new(),
        access: AccessLevel::Public,
        specifiers: Specifiers::default(),
        doc_comment: None,
    }
}

#[cfg(test)]
mod tests;
