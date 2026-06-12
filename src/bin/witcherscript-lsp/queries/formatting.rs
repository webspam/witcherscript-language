use std::time::Instant;

use async_lsp::ResponseError;
use lsp_types::{DocumentFormattingParams, TextEdit};

use tracing::trace;
use witcherscript_language::builtins::builtin_source;
use witcherscript_language::formatter::format_document;

use crate::backend::Backend;
use crate::convert::lsp_range;

type Result<T> = std::result::Result<T, ResponseError>;

impl Backend {
    pub(crate) fn _formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return Ok(None);
        }
        let started_at = Instant::now();
        trace!(op = "formatting", uri = %uri, "start");
        let result = 'body: {
            let tab_size = params.options.tab_size;
            let use_tabs = !params.options.insert_spaces;

            // Include a queued edit: clients don't retry formatting on CONTENT_MODIFIED, so
            // bailing would silently apply nothing instead of formatting the just-typed text.
            let Some(document_arc) = self.latest_parsed_document(&uri, &self.snapshot()) else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();

            let formatted = format_document(
                document.tree.root_node(),
                &document.source,
                self.format_options(use_tabs, tab_size),
            );

            if formatted == document.source {
                break 'body Ok(Some(Vec::new()));
            }

            let full_range = lsp_range(document.line_index.byte_range_to_range(
                &document.source,
                0,
                document.source.len(),
            ));

            Ok(Some(vec![TextEdit {
                range: full_range,
                new_text: formatted,
            }]))
        };
        trace!(
            op = "formatting",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
