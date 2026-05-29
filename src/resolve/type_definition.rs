use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

use super::definition::resolve_definition;
use super::symbol_db::SymbolDb;
use super::{parse_generic_type, Definition};

pub fn resolve_type_definition(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Definition> {
    let def = resolve_definition(uri, document, db, position)?;
    type_target_for(&def, db)
}

fn type_target_for(def: &Definition, db: &SymbolDb<'_>) -> Option<Definition> {
    match def.symbol.kind {
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum | SymbolKind::State => {
            Some(def.clone())
        }
        SymbolKind::EnumMember => {
            let owner = def.symbol.container_name.as_deref()?;
            db.find_top_level(owner)
        }
        _ => {
            let raw = def.symbol.type_annotation.as_deref()?;
            let lookup = parse_generic_type(raw).map(|(ctor, _)| ctor).unwrap_or(raw);
            db.find_top_level(lookup)
        }
    }
}
