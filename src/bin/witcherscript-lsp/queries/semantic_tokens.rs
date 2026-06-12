use std::sync::atomic::Ordering;
use std::time::Instant;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::{
    SemanticTokens, SemanticTokensDelta, SemanticTokensDeltaParams, SemanticTokensFullDeltaResult,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, Url,
};

use tracing::trace;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::line_index::SourceRange;
use witcherscript_language::semantic_tokens::{
    collect_semantic_tokens_cancellable, collect_semantic_tokens_in_range_cancellable,
};

use crate::backend::{Backend, Result};
use crate::convert::{source_position, source_range};
use crate::semantic_tokens_cache::{
    CachedSemanticTokens, semantic_token_edits, semantic_token_structs,
};

impl Backend {
    fn computed_semantic_tokens(
        &self,
        uri: &Url,
        range: Option<SourceRange>,
    ) -> Result<Option<Vec<u32>>> {
        let snap = self.snapshot();
        let Some(document_arc) = snap.documents.get(uri).cloned() else {
            return Ok(None);
        };
        let document = document_arc.as_ref();
        let target = self.pending_target_for(uri).unwrap_or(0);
        if target > document.parse_version {
            return Err(ResponseError::new(
                ErrorCode::CONTENT_MODIFIED,
                "document edited while computing semantic tokens",
            ));
        }
        let handles = self.db_handles_for_with_snapshot(uri, &snap);
        let db = handles.db();
        let version = self.state_version.load(Ordering::Acquire);
        let state_version = self.state_version.clone();
        let should_continue = || state_version.load(Ordering::Acquire) == version;
        let canonical = canonical_uri(uri);
        let data = match range {
            Some(range) => collect_semantic_tokens_in_range_cancellable(
                &canonical,
                document,
                &db,
                range,
                &should_continue,
            ),
            None => {
                collect_semantic_tokens_cancellable(&canonical, document, &db, &should_continue)
            }
        };
        let Some(data) = data else {
            return Err(ResponseError::new(
                ErrorCode::CONTENT_MODIFIED,
                "document changed while computing semantic tokens",
            ));
        };
        Ok(Some(data))
    }

    pub(crate) fn _semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        trace!(op = "semantic_tokens_full", uri = %uri, "start");
        let result = match self.computed_semantic_tokens(&uri, None) {
            Ok(Some(data)) => {
                let result_id = self.next_semantic_tokens_result_id();
                let tokens = semantic_token_structs(&data);
                self.semantic_tokens_cache.lock().insert(
                    uri.clone(),
                    CachedSemanticTokens {
                        result_id: result_id.clone(),
                        data,
                    },
                );
                Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                    result_id: Some(result_id),
                    data: tokens,
                })))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        };
        trace!(
            op = "semantic_tokens_full",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) fn _semantic_tokens_full_delta(
        &self,
        params: SemanticTokensDeltaParams,
    ) -> Result<Option<SemanticTokensFullDeltaResult>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        trace!(op = "semantic_tokens_full_delta", uri = %uri, "start");
        let result = match self.computed_semantic_tokens(&uri, None) {
            Ok(Some(data)) => {
                let result_id = self.next_semantic_tokens_result_id();
                let mut cache = self.semantic_tokens_cache.lock();
                let response = match cache.get(&uri) {
                    Some(previous) if previous.result_id == params.previous_result_id => {
                        SemanticTokensFullDeltaResult::TokensDelta(SemanticTokensDelta {
                            result_id: Some(result_id.clone()),
                            edits: semantic_token_edits(&previous.data, &data),
                        })
                    }
                    // Unknown previous_result_id: protocol says answer with a full payload.
                    _ => SemanticTokensFullDeltaResult::Tokens(SemanticTokens {
                        result_id: Some(result_id.clone()),
                        data: semantic_token_structs(&data),
                    }),
                };
                cache.insert(uri.clone(), CachedSemanticTokens { result_id, data });
                Ok(Some(response))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        };
        trace!(
            op = "semantic_tokens_full_delta",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    fn next_semantic_tokens_result_id(&self) -> String {
        self.semantic_tokens_result_id
            .fetch_add(1, Ordering::Relaxed)
            .to_string()
    }

    pub(crate) fn _semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        trace!(op = "semantic_tokens_range", uri = %uri, "start");
        let range = source_range(
            source_position(params.range.start),
            source_position(params.range.end),
        );
        let result = match self.computed_semantic_tokens(&uri, Some(range)) {
            Ok(Some(data)) => Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
                result_id: None,
                data: semantic_token_structs(&data),
            }))),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        };
        trace!(
            op = "semantic_tokens_range",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
