use std::time::Instant;

use lsp_types::{CodeActionParams, CodeActionResponse, CodeActionTriggerKind, Range, Url};

use tracing::trace;
use witcherscript_language::files::canonical_uri;

use crate::backend::{Backend, Result};
use crate::convert::{
    base_script_conflict_code_actions, refactor_code_actions, remove_unused_code_actions,
    source_position,
};

impl Backend {
    pub(crate) fn _code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "code_action", uri = %uri, "start");
        let roots = self.workspace_roots.load_full();
        let mut actions = base_script_conflict_code_actions(&params.context.diagnostics, &roots);
        // An Automatic trigger is the editor requesting code actions on its own, not the user asking
        if params.context.trigger_kind != Some(CodeActionTriggerKind::AUTOMATIC) {
            actions.extend(self.refactor_actions(&uri, params.range));
        }
        // Remove-unused is the lowest-priority fix, so it always trails the rest of the list.
        actions.extend(remove_unused_code_actions(
            &uri,
            &params.context.diagnostics,
        ));
        trace!(
            op = "code_action",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        Ok((!actions.is_empty()).then_some(actions))
    }

    fn refactor_actions(&self, uri: &Url, range: Range) -> CodeActionResponse {
        let snap = self.snapshot();
        let Some(document_arc) = self.latest_parsed_document(uri, &snap) else {
            return Vec::new();
        };
        let document = document_arc.as_ref();
        let to_byte = |position| {
            document
                .line_index
                .position_to_byte(&document.source, source_position(position))
        };
        let (Some(start), Some(end)) = (to_byte(range.start), to_byte(range.end)) else {
            return Vec::new();
        };
        let handles = self.db_handles_for_with_snapshot(uri, &snap);
        let db = handles.db();
        let canonical = canonical_uri(uri);
        let cfg = self.config.load();
        let options = self.format_options(uri, !cfg.editor_insert_spaces, cfg.editor_tab_size);
        refactor_code_actions(uri, &canonical, document, &db, start..end, options)
    }
}
