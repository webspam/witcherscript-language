use std::sync::Arc;

use witcherscript_language::resolve::{Definition, SymbolDb, WorkspaceIndex};
use witcherscript_language::script_env::ScriptEnvironment;

#[derive(Debug, Clone)]
pub(crate) struct MergedCompletionCache {
    pub workspace_surface: u64,
    pub base_surface: u64,
    pub script_env_version: u64,
    pub callables: Arc<[Definition]>,
    pub types: Arc<[Definition]>,
    pub enum_variants: Arc<[Definition]>,
    pub script_globals: Arc<[Definition]>,
}

impl MergedCompletionCache {
    pub(crate) fn build(
        workspace: &WorkspaceIndex,
        base: &WorkspaceIndex,
        db: &SymbolDb,
        script_env: &ScriptEnvironment,
    ) -> Self {
        let script_globals: Arc<[Definition]> = Arc::from(
            script_env
                .globals
                .iter()
                .map(|g| Definition {
                    uri: g.ini_uri.clone(),
                    symbol: g.symbol.clone(),
                })
                .collect::<Vec<_>>(),
        );
        Self {
            workspace_surface: workspace.surface_hash(),
            base_surface: base.surface_hash(),
            script_env_version: script_env.version(),
            callables: db.merged_callables_catalog(),
            types: db.merged_types_catalog(),
            enum_variants: db.merged_enum_variants_catalog(),
            script_globals,
        }
    }

    pub(crate) fn globals(&self) -> Vec<Definition> {
        let mut out = Vec::with_capacity(
            self.callables.len() + self.script_globals.len() + self.enum_variants.len(),
        );
        out.extend(self.callables.iter().cloned());
        out.extend(self.script_globals.iter().cloned());
        out.extend(self.enum_variants.iter().cloned());
        out
    }
}
