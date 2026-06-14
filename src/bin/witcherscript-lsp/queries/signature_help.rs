use std::time::Instant;

use lsp_types::{SignatureHelp, SignatureHelpParams};

use tracing::trace;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::signature_help;

use crate::backend::{Backend, Result};
use crate::convert::{signature_help_response, source_position};

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
            let colon = self.config.load().colon_spacing();

            Ok(signature_help(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
                colon,
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
