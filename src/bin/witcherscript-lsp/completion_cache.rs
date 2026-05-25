use std::sync::Arc;

use witcherscript_language::resolve::{
    merged_global_completions, Definition, SymbolDb, WorkspaceIndex,
};
use witcherscript_language::script_env::ScriptEnvironment;

#[derive(Debug, Clone)]
pub(crate) struct MergedCompletionCache {
    pub workspace_surface: u64,
    pub base_surface: u64,
    pub script_env_version: u64,
    pub types: Arc<[Definition]>,
    pub globals: Arc<[Definition]>,
}

impl MergedCompletionCache {
    pub(crate) fn build(
        workspace: &WorkspaceIndex,
        base: &WorkspaceIndex,
        db: &SymbolDb,
        script_env: &ScriptEnvironment,
    ) -> Self {
        Self {
            workspace_surface: workspace.surface_hash(),
            base_surface: base.surface_hash(),
            script_env_version: script_env.version(),
            types: db.merged_types_catalog(),
            globals: Arc::from(merged_global_completions(db)),
        }
    }

    pub(crate) fn globals(&self) -> &[Definition] {
        &self.globals
    }
}
