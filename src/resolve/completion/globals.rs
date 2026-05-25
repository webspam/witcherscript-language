use super::super::symbol_db::SymbolDb;
use super::super::Definition;

pub fn global_body_completions(db: &SymbolDb) -> Vec<Definition> {
    let mut globals = db.all_top_level_callables();
    globals.extend(db.all_script_globals());
    globals.extend(db.all_enum_variants());
    globals
}
