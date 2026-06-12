use std::time::Instant;

use async_lsp::ResponseError;
use lsp_types::{DocumentSymbolParams, DocumentSymbolResponse};

use tracing::trace;

use crate::backend::Backend;
use crate::convert::document_symbols;

type Result<T> = std::result::Result<T, ResponseError>;

impl Backend {
    pub(crate) fn _document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "document_symbol", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };

            Ok(Some(DocumentSymbolResponse::Nested(document_symbols(
                &document.symbols,
                None,
            ))))
        };
        trace!(
            op = "document_symbol",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
