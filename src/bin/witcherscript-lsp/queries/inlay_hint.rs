use std::sync::atomic::Ordering;
use std::time::Instant;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::{InlayHint, InlayHintParams};

use tracing::trace;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::inlay_hints;

use crate::backend::Backend;
use crate::convert::{inlay_hint, source_position, source_range};

type Result<T> = std::result::Result<T, ResponseError>;

impl Backend {
    pub(crate) fn _inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        trace!(op = "inlay_hint", uri = %uri, "start");
        let result = 'body: {
            if !self.config.load().inlay_hints {
                break 'body Ok(None);
            }
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let target = self.pending_target_for(&uri).unwrap_or(0);
            if target > document.parse_version {
                break 'body Err(ResponseError::new(
                    ErrorCode::CONTENT_MODIFIED,
                    "document edited while computing inlay hints",
                ));
            }
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let version = self.state_version.load(Ordering::Acquire);
            let state_version = self.state_version.clone();
            let should_continue = || state_version.load(Ordering::Acquire) == version;
            let range = source_range(
                source_position(params.range.start),
                source_position(params.range.end),
            );
            let Some(infos) =
                inlay_hints(&canonical_uri(&uri), document, &db, range, &should_continue)
            else {
                break 'body Err(ResponseError::new(
                    ErrorCode::CONTENT_MODIFIED,
                    "document changed while computing inlay hints",
                ));
            };
            Ok(Some(infos.into_iter().map(inlay_hint).collect()))
        };
        trace!(
            op = "inlay_hint",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
