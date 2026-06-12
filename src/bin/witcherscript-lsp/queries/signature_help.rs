use std::time::Instant;

use async_lsp::ResponseError;
use lsp_types::{SignatureHelp, SignatureHelpParams};

use tracing::trace;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::signature_help;

use crate::backend::Backend;
use crate::convert::{signature_help_response, source_position};

type Result<T> = std::result::Result<T, ResponseError>;

impl Backend {
    pub(crate) fn _signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "signature_help", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let compact_colon = self.config.load().formatter_compact_colon;

            Ok(signature_help(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
                compact_colon,
            )
            .map(signature_help_response))
        };
        trace!(
            op = "signature_help",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
