use std::time::Instant;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::{CodeLens, CodeLensParams, Command, Location, Position, Url};

use tracing::{trace, warn};
use witcherscript_language::resolve::{OverriddenSymbol, overridden_top_level};
use witcherscript_language::symbols::{Symbol, SymbolKind};

use super::ReferenceLensData;
use crate::backend::{Backend, Result};
use crate::convert::lsp_range;

const GO_TO_BASE_COMMAND: &str = "witcherscript.goToBaseDefinition";
const GO_TO_BASE_TITLE: &str = "game definition";
const SHOW_REFERENCES_COMMAND: &str = "witcherscript.showReferences";

// Custom command, not a built-in: VS Code built-ins reject raw JSON args, so the extension wrapper reconstructs vscode types from this Location.
fn base_definition_lens(overridden: &OverriddenSymbol) -> Option<CodeLens> {
    let uri = match Url::parse(&overridden.base.uri) {
        Ok(uri) => uri,
        Err(err) => {
            warn!(uri = %overridden.base.uri, %err, "base symbol uri failed to parse; skipping lens");
            return None;
        }
    };
    let target = Location {
        uri,
        range: lsp_range(overridden.base.symbol.selection_range),
    };
    let argument = serde_json::to_value(target).expect("Location always serializes");
    Some(CodeLens {
        range: lsp_range(overridden.range),
        command: Some(Command {
            title: GO_TO_BASE_TITLE.to_string(),
            command: GO_TO_BASE_COMMAND.to_string(),
            arguments: Some(vec![argument]),
        }),
        data: None,
    })
}

fn symbol_eligible_for_reference_lens(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Function
            | SymbolKind::State
            | SymbolKind::Method
            | SymbolKind::Event
    )
}

fn reference_lens(symbol: &Symbol, uri: &Url) -> CodeLens {
    let range = lsp_range(symbol.selection_range);
    let data = ReferenceLensData {
        uri: uri.clone(),
        position: range.start,
    };
    CodeLens {
        range,
        command: None,
        data: Some(serde_json::to_value(data).expect("ReferenceLensData always serializes")),
    }
}

impl Backend {
    pub(crate) fn _code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri;
        let started_at = Instant::now();
        trace!(op = "code_lens", uri = %uri, "start");
        let result = 'body: {
            let cfg = self.config.load();
            let want_overrides = cfg.code_lens_overridden_symbols;
            let want_references = cfg.code_lens_references;
            if !want_overrides && !want_references {
                trace!(op = "code_lens", uri = %uri, reason = "feature_disabled", "skip");
                break 'body Ok(None);
            }
            let snap = self.snapshot();
            let Some(document) = snap.documents.get(&uri).cloned() else {
                trace!(op = "code_lens", uri = %uri, reason = "no_open_document", "skip");
                break 'body Ok(None);
            };
            let mut lenses: Vec<CodeLens> = Vec::new();
            // References first so it keeps a fixed left position; the optional game-def lens renders to its right.
            if want_references {
                lenses.extend(
                    document
                        .symbols
                        .all()
                        .iter()
                        .filter(|s| symbol_eligible_for_reference_lens(s))
                        .map(|s| reference_lens(s, &uri)),
                );
            }
            if want_overrides && self.replaces_base_script(&uri) {
                lenses.extend(
                    overridden_top_level(document.symbols.all(), &snap.base_scripts_index)
                        .into_iter()
                        .filter_map(|o| base_definition_lens(&o)),
                );
            }
            trace!(
                op = "code_lens",
                uri = %uri,
                base_docs = snap.base_scripts_index.documents().count(),
                lenses = lenses.len(),
                "computed",
            );
            Ok((!lenses.is_empty()).then_some(lenses))
        };
        trace!(
            op = "code_lens",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }

    pub(crate) async fn _code_lens_resolve(&self, mut lens: CodeLens) -> Result<CodeLens> {
        // Game-definition lenses arrive fully built (command set, no data); pass them through.
        let Some(data) = lens.data.take() else {
            return Ok(lens);
        };
        let ReferenceLensData { uri, position } = serde_json::from_value(data).map_err(|err| {
            ResponseError::new(
                ErrorCode::INVALID_PARAMS,
                format!("malformed reference code-lens data: {err}"),
            )
        })?;
        // No index wait: parked resolves can fill the request cap and deadlock; the post-index CodeLensRefresh corrects any undercount.
        self.spawn_compute(move |b| b._code_lens_resolve_blocking(lens, &uri, position))
            .await
    }

    pub(crate) fn _code_lens_resolve_blocking(
        &self,
        mut lens: CodeLens,
        uri: &Url,
        position: Position,
    ) -> Result<CodeLens> {
        let started_at = Instant::now();
        trace!(op = "code_lens_resolve", uri = %uri, "start");
        let locations = self
            .reference_locations(uri, position, false)
            .unwrap_or_default();
        let count = locations.len();
        let title = if count == 1 {
            "1 reference".to_string()
        } else {
            format!("{count} references")
        };
        let arguments = vec![
            serde_json::to_value(uri).expect("Url always serializes"),
            serde_json::to_value(position).expect("Position always serializes"),
            serde_json::to_value(&locations).expect("Locations always serialize"),
        ];
        lens.command = Some(Command {
            title,
            command: SHOW_REFERENCES_COMMAND.to_string(),
            arguments: Some(arguments),
        });
        trace!(
            op = "code_lens_resolve",
            uri = %uri,
            count,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        Ok(lens)
    }
}
