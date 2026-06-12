use std::time::Instant;

use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse, Location, Url};

use tracing::trace;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::{resolve_all_definitions, resolve_type_definition};

use crate::backend::{Backend, Result};
use crate::convert::{lsp_range, source_position};

impl Backend {
    pub(crate) fn _definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "definition", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let definitions = resolve_all_definitions(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
            );

            let locations: Vec<Location> = definitions
                .into_iter()
                .filter_map(|definition| {
                    Url::parse(&definition.uri).ok().map(|target_uri| Location {
                        uri: target_uri,
                        range: lsp_range(definition.symbol.selection_range),
                    })
                })
                .collect();

            match locations.as_slice() {
                [] => Ok(None),
                [single] => Ok(Some(GotoDefinitionResponse::Scalar(single.clone()))),
                _ => Ok(Some(GotoDefinitionResponse::Array(locations))),
            }
        };
        trace!(
            op = "definition",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) fn _type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let started_at = Instant::now();
        trace!(op = "type_definition", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();

            let Some(type_def) = resolve_type_definition(
                &canonical_uri(&uri),
                document,
                &db,
                source_position(position),
            ) else {
                break 'body Ok(None);
            };

            let Ok(target_uri) = Url::parse(&type_def.uri) else {
                break 'body Ok(None);
            };
            Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: target_uri,
                range: lsp_range(type_def.symbol.selection_range),
            })))
        };
        trace!(
            op = "type_definition",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
