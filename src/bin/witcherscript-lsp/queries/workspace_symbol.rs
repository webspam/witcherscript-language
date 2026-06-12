use std::time::Instant;

use async_lsp::ResponseError;
use lsp_types::{WorkspaceSymbolParams, WorkspaceSymbolResponse};

use tracing::trace;
use witcherscript_language::resolve::workspace_symbols;

use crate::backend::Backend;
use crate::convert::workspace_symbol;

type Result<T> = std::result::Result<T, ResponseError>;

const MAX_WORKSPACE_SYMBOL_RESULTS: usize = 256;

impl Backend {
    pub(crate) fn _workspace_symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        let started_at = Instant::now();
        trace!(op = "workspace_symbol", query = %params.query, "start");
        let snap = self.snapshot();
        let indexes = [
            snap.workspace_index.as_ref(),
            snap.base_scripts_index.as_ref(),
            self.builtins_index.as_ref(),
        ];
        let matches = workspace_symbols(
            &indexes,
            &params.query,
            MAX_WORKSPACE_SYMBOL_RESULTS,
            Some(snap.suppressed_base_uris.as_ref()),
        );
        let symbols: Vec<_> = matches.iter().filter_map(workspace_symbol).collect();
        trace!(
            op = "workspace_symbol",
            query = %params.query,
            count = symbols.len(),
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        Ok(Some(WorkspaceSymbolResponse::Nested(symbols)))
    }
}
