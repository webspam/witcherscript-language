use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::types::parse_generic_type;

use super::definition::resolve_definition;
use super::inference::definition_type_name;
use super::symbol_db::SymbolDb;
use super::Definition;

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
    if def.symbol.kind.is_type() {
        return Some(def.clone());
    }
    let raw = definition_type_name(def)?;
    let lookup = parse_generic_type(&raw)
        .map(|(ctor, _)| ctor)
        .unwrap_or(raw.as_str());
    db.find_top_level(lookup)
}
