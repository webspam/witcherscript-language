use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;

use super::Definition;
use super::definition::resolve_definition;
use super::inference::definition_type;
use super::symbol_db::SymbolDb;

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
    let lookup = definition_type(def)?.to_lookup_ctor()?;
    db.find_top_level(&lookup)
}
