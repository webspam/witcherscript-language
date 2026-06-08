use super::super::Definition;
use super::super::symbol_db::SymbolDb;

pub fn merged_global_completions(db: &SymbolDb) -> Vec<Definition> {
    let mut globals = Vec::with_capacity(
        db.merged_callables_catalog().len()
            + db.all_script_globals().len()
            + db.merged_enum_members_catalog().len(),
    );
    globals.extend(db.merged_callables_catalog().iter().cloned());
    globals.extend(db.all_script_globals());
    globals.extend(db.merged_enum_members_catalog().iter().cloned());
    globals
}
