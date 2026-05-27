use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::{
    Location, PrepareRenameResponse, ReferenceParams, RenameParams, TextDocumentPositionParams,
    TextEdit, Url, WorkspaceEdit,
};
use tracing::{info, trace};
use witcherscript_language::builtins::builtin_source;
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::resolve::{find_references, resolve_definition};

use crate::backend::Backend;
use crate::convert::{lsp_range, source_position};
use crate::logging::wall_clock_us;

type Result<T> = std::result::Result<T, ResponseError>;

// Open editor docs shadow workspace docs which shadow base docs - unsaved edits win.
// Loose files form a compilation isolated from the workspace, so a search whose
// target is loose sees base + loose docs, and a workspace search excludes loose docs.
pub(crate) fn merge_documents<'a>(
    base_docs: &'a HashMap<String, Arc<ParsedDocument>>,
    workspace_docs: &'a HashMap<String, Arc<ParsedDocument>>,
    open_documents: &'a HashMap<Url, Arc<ParsedDocument>>,
    open_loose_uris: &HashSet<Url>,
    target_is_loose: bool,
) -> HashMap<String, &'a ParsedDocument> {
    let mut merged: HashMap<String, &ParsedDocument> = HashMap::new();
    for (uri, doc) in base_docs.iter() {
        merged.insert(uri.clone(), doc.as_ref());
    }
    if !target_is_loose {
        for (uri, doc) in workspace_docs.iter() {
            merged.insert(uri.clone(), doc.as_ref());
        }
    }
    for (url, doc) in open_documents.iter() {
        if open_loose_uris.contains(url) == target_is_loose {
            merged.insert(url.to_string(), doc.as_ref());
        }
    }
    merged
}

// Base scripts are read-only: references found inside them must never become edits,
// even when the renamed symbol's declaration lives in the workspace (e.g. an
// @wrapMethod whose target's class-body declaration sits in a base script).
pub(crate) fn rename_changes(
    refs: &[(String, witcherscript_language::line_index::SourceRange)],
    new_name: &str,
    base_docs: &HashMap<String, Arc<ParsedDocument>>,
) -> HashMap<Url, Vec<TextEdit>> {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for (ref_uri, range) in refs {
        if base_docs.contains_key(ref_uri) || builtin_source(ref_uri).is_some() {
            continue;
        }
        if let Ok(url) = Url::parse(ref_uri) {
            changes.entry(url).or_default().push(TextEdit {
                range: lsp_range(*range),
                new_text: new_name.to_string(),
            });
        }
    }
    changes
}

impl Backend {
    pub(crate) async fn _references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;
        let started_at = Instant::now();
        trace!(op = "references", uri = %uri, at = %wall_clock_us(), "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();

            let ws_kb = handles.workspace().doc_idents_bytes() / 1024;
            let base_kb = handles.base().doc_idents_bytes() / 1024;
            info!(
                ws_kb,
                base_kb,
                total_kb = ws_kb + base_kb,
                "ident index memory"
            );

            let Some(definition) =
                resolve_definition(uri.as_str(), document, &db, source_position(position))
            else {
                break 'body Ok(Some(Vec::new()));
            };

            let loose_uris = self.loose_open_uris(&snap.documents);
            let target_is_loose = loose_uris.contains(&uri);

            let merged = merge_documents(
                &snap.base_scripts_documents,
                &snap.workspace_documents,
                &snap.documents,
                &loose_uris,
                target_is_loose,
            );

            let definition_document = merged.get(&definition.uri).copied().unwrap_or(document);

            let search_docs: Vec<(&str, &ParsedDocument)> = merged
                .iter()
                .map(|(uri, doc)| (uri.as_str(), *doc))
                .collect();

            let refs = find_references(
                &definition,
                definition_document,
                &search_docs,
                &db,
                include_declaration,
            );

            let locations: Vec<Location> = refs
                .into_iter()
                .filter_map(|(ref_uri, range)| {
                    Url::parse(&ref_uri).ok().map(|url| Location {
                        uri: url,
                        range: lsp_range(range),
                    })
                })
                .collect();

            Ok(Some(locations))
        };
        trace!(
            op = "references",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
            "complete",
        );
        result
    }

    pub(crate) async fn _prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "prepare_rename", uri = %uri, at = %wall_clock_us(), "start");
        let result = 'body: {
            let Some(definition) = self.resolve_at(&uri, params.position) else {
                break 'body Ok(None);
            };

            let snap = self.snapshot();
            if snap.base_scripts_documents.contains_key(&definition.uri) {
                break 'body Err(ResponseError::new(
                    ErrorCode::INVALID_REQUEST,
                    "Cannot rename a symbol declared in a base script (read-only)",
                ));
            }
            if builtin_source(&definition.uri).is_some() {
                break 'body Err(ResponseError::new(
                    ErrorCode::INVALID_REQUEST,
                    "Cannot rename a built-in symbol",
                ));
            }

            Ok(Some(PrepareRenameResponse::DefaultBehavior {
                default_behavior: true,
            }))
        };
        trace!(
            op = "prepare_rename",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
            "complete",
        );
        result
    }

    pub(crate) async fn _rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let new_name = params.new_name;
        let started_at = Instant::now();
        trace!(op = "rename", uri = %uri, at = %wall_clock_us(), "start");
        let result = 'body: {
            let Some(definition) = self.resolve_at(&uri, params.text_document_position.position)
            else {
                break 'body Ok(None);
            };

            let snap = self.snapshot();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);

            if snap.base_scripts_documents.contains_key(&definition.uri) {
                break 'body Err(ResponseError::new(
                    ErrorCode::INVALID_REQUEST,
                    "Cannot rename a symbol declared in a base script (read-only)",
                ));
            }
            if builtin_source(&definition.uri).is_some() {
                break 'body Err(ResponseError::new(
                    ErrorCode::INVALID_REQUEST,
                    "Cannot rename a built-in symbol",
                ));
            }

            let db = handles.db();

            let loose_uris = self.loose_open_uris(&snap.documents);
            let merged = merge_documents(
                &snap.base_scripts_documents,
                &snap.workspace_documents,
                &snap.documents,
                &loose_uris,
                loose_uris.contains(&uri),
            );

            let Some(definition_document) = merged.get(&definition.uri).copied() else {
                break 'body Ok(None);
            };

            let search_docs: Vec<(&str, &ParsedDocument)> = merged
                .iter()
                .map(|(uri, doc)| (uri.as_str(), *doc))
                .collect();

            let refs = find_references(&definition, definition_document, &search_docs, &db, true);

            let changes = rename_changes(&refs, &new_name, &snap.base_scripts_documents);

            Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            }))
        };
        trace!(
            op = "rename",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
            "complete",
        );
        result
    }
}
