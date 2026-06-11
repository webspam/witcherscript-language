use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::{
    Location, Position, PrepareRenameResponse, ReferenceParams, RenameParams,
    TextDocumentPositionParams, TextEdit, Url, WorkspaceEdit,
};
use tracing::{debug, trace};
use witcherscript_language::builtins::builtin_source;
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::{find_references, resolve_definition};

use crate::backend::Backend;
use crate::compilation::Compilation;
use crate::convert::{lsp_range, source_position};

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
    for (uri, doc) in base_docs {
        merged.insert(uri.clone(), doc.as_ref());
    }
    if !target_is_loose {
        for (uri, doc) in workspace_docs {
            merged.insert(uri.clone(), doc.as_ref());
        }
    }
    for (url, doc) in open_documents {
        if open_loose_uris.contains(url) == target_is_loose {
            merged.insert(canonical_uri(url), doc.as_ref());
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

fn ensure_rename_target_writable(definition_uri: &str, snap: &Compilation) -> Result<()> {
    if snap.base_scripts_documents.contains_key(definition_uri) {
        return Err(ResponseError::new(
            ErrorCode::INVALID_REQUEST,
            "Cannot rename a symbol declared in a base script (read-only)",
        ));
    }
    if builtin_source(definition_uri).is_some() {
        return Err(ResponseError::new(
            ErrorCode::INVALID_REQUEST,
            "Cannot rename a built-in symbol",
        ));
    }
    Ok(())
}

fn search_docs_from<'a>(
    merged: &'a HashMap<String, &'a ParsedDocument>,
) -> Vec<(&'a str, &'a ParsedDocument)> {
    merged
        .iter()
        .map(|(uri, doc)| (uri.as_str(), *doc))
        .collect()
}

impl Backend {
    // `None` means the document is not open; `Some(empty)` means it resolved to nothing.
    pub(crate) fn reference_locations(
        &self,
        uri: &Url,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        let snap = self.snapshot();
        let document_arc = snap.documents.get(uri).cloned()?;
        let document = document_arc.as_ref();
        let handles = self.db_handles_for_with_snapshot(uri, &snap);
        let db = handles.db();

        let ws_kb = handles.workspace().doc_idents_bytes() / 1024;
        let base_kb = handles.base().doc_idents_bytes() / 1024;
        debug!(
            ws_kb,
            base_kb,
            total_kb = ws_kb + base_kb,
            "ident index memory"
        );

        let Some(definition) = resolve_definition(
            &canonical_uri(uri),
            document,
            &db,
            source_position(position),
        ) else {
            return Some(Vec::new());
        };

        let loose_uris = self.loose_open_uris(&snap.documents);
        let target_is_loose = loose_uris.contains(uri);

        let merged = merge_documents(
            &snap.base_scripts_documents,
            &snap.workspace_documents,
            &snap.documents,
            &loose_uris,
            target_is_loose,
        );

        let definition_document = merged.get(&definition.uri).copied().unwrap_or(document);

        let search_docs = search_docs_from(&merged);

        let refs = find_references(
            &definition,
            definition_document,
            &search_docs,
            &db,
            include_declaration,
        );

        Some(
            refs.into_iter()
                .filter_map(|(ref_uri, range)| {
                    Url::parse(&ref_uri).ok().map(|url| Location {
                        uri: url,
                        range: lsp_range(range),
                    })
                })
                .collect(),
        )
    }

    pub(crate) fn _references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;
        let started_at = Instant::now();
        trace!(op = "references", uri = %uri, "start");
        let result = Ok(self.reference_locations(&uri, position, include_declaration));
        trace!(
            op = "references",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) fn _prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri.clone();
        let started_at = Instant::now();
        trace!(op = "prepare_rename", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(definition) = self.resolve_at(&uri, params.position, &snap) else {
                break 'body Ok(None);
            };

            if let Err(e) = ensure_rename_target_writable(&definition.uri, &snap) {
                break 'body Err(e);
            }

            Ok(Some(PrepareRenameResponse::DefaultBehavior {
                default_behavior: true,
            }))
        };
        trace!(
            op = "prepare_rename",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) fn _rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let new_name = params.new_name;
        let started_at = Instant::now();
        trace!(op = "rename", uri = %uri, "start");
        let result = 'body: {
            let snap = self.snapshot();
            let Some(definition) =
                self.resolve_at(&uri, params.text_document_position.position, &snap)
            else {
                break 'body Ok(None);
            };

            let handles = self.db_handles_for_with_snapshot(&uri, &snap);

            if let Err(e) = ensure_rename_target_writable(&definition.uri, &snap) {
                break 'body Err(e);
            }

            let db = handles.db();

            let loose_uris = self.loose_open_uris(&snap.documents);
            // A queued edit isn't in the snapshot yet; search the pending copy so rename sees just-applied text.
            let latest = self.latest_parsed_document(&uri, &snap);
            let mut merged = merge_documents(
                &snap.base_scripts_documents,
                &snap.workspace_documents,
                &snap.documents,
                &loose_uris,
                loose_uris.contains(&uri),
            );
            if let Some(doc) = latest.as_deref() {
                merged.insert(canonical_uri(&uri), doc);
            }

            let Some(definition_document) = merged.get(&definition.uri).copied() else {
                break 'body Ok(None);
            };

            let search_docs = search_docs_from(&merged);

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
            "complete",
        );
        result
    }
}
