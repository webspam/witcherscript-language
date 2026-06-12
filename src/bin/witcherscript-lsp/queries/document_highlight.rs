use std::time::Instant;

use async_lsp::ResponseError;
use lsp_types::{DocumentHighlight, DocumentHighlightParams};

use tracing::trace;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::document_highlights;

use crate::backend::Backend;
use crate::convert::{document_highlight, source_position};

type Result<T> = std::result::Result<T, ResponseError>;

impl Backend {
    pub(crate) fn _document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "document_highlight", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let highlights = document_highlights(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
            )
            .map(|hits| {
                hits.into_iter()
                    .map(|(range, kind)| document_highlight(range, kind))
                    .collect()
            });
            Ok(highlights)
        };
        trace!(
            op = "document_highlight",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
