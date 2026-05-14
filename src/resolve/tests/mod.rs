mod chaining;
mod completion_annotation;
mod completion_keywords;
mod completion_members;
mod completion_script_keywords;
mod completion_statement;
mod completion_type;
mod definition;
mod index;
mod inheritance;
mod parameters;
mod references;
mod script_globals;
mod signature_help;

use super::{SymbolDb, WorkspaceIndex};
use crate::document::{parse_document, ParsedDocument};
use crate::line_index::SourcePosition;
use crate::script_env::ScriptEnvironment;
use crate::symbols::AccessLevel;

pub(super) fn make_doc(source: &str) -> ParsedDocument {
    parse_document(source).expect("parse should succeed")
}

#[allow(dead_code)]
pub(super) fn make_index(uri: &str, doc: &ParsedDocument) -> WorkspaceIndex {
    let mut idx = WorkspaceIndex::default();
    idx.update_document(uri, doc);
    idx
}

pub(super) fn make_env(name: &str, type_name: &str) -> ScriptEnvironment {
    use crate::line_index::SourceRange;
    use crate::script_env::ScriptGlobal;
    use crate::symbols::{Symbol, SymbolId, SymbolKind};
    let pos = SourcePosition {
        line: 1,
        character: 0,
    };
    let end = SourcePosition {
        line: 1,
        character: name.len() as u32,
    };
    ScriptEnvironment {
        globals: vec![ScriptGlobal {
            name: name.to_string(),
            type_name: type_name.to_string(),
            ini_uri: "file:///redscripts.ini".to_string(),
            symbol: Symbol {
                id: SymbolId(0),
                name: name.to_string(),
                kind: SymbolKind::Variable,
                range: SourceRange { start: pos, end },
                selection_range: SourceRange { start: pos, end },
                byte_range: 0..name.len(),
                selection_byte_range: 0..name.len(),
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
        }],
    }
}
