use std::time::Instant;

use lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};

use tracing::trace;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::resolve_definition;

use crate::backend::{Backend, Result};
use crate::convert::{hover_markdown, lsp_range, source_position};

impl Backend {
    pub(crate) fn _hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "hover", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let Some(definition) = resolve_definition(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
            ) else {
                break 'body Ok(None);
            };

            Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: hover_markdown(&definition, &db),
                }),
                range: Some(lsp_range(definition.symbol.selection_range)),
            }))
        };
        trace!(
            op = "hover",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
