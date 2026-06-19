use lsp_types::{Position, Url};
use tracing::warn;
use witcherscript_language::format_config;
use witcherscript_language::formatter::FormatOptions;

use crate::backend::Backend;

mod code_action;
mod code_lens;
mod definition;
mod diagnostics;
mod document_highlight;
mod document_symbol;
mod formatting;
mod hover;
mod inlay_hint;
mod semantic_tokens;
mod signature_help;
mod workspace_symbol;

// Identifies the declaration a reference-count lens belongs to so phase 2 can re-resolve it.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ReferenceLensData {
    pub(crate) uri: Url,
    pub(crate) position: Position,
}

impl Backend {
    // A `.wsformat.toml` beside the document overrides editor settings field-by-field.
    fn format_options(&self, uri: &Url, use_tabs: bool, tab_size: u32) -> FormatOptions {
        let cfg = self.config.load();
        let base = FormatOptions {
            tab_size,
            use_tabs,
            line_limit: cfg.formatter_line_limit,
            colon: cfg.colon_spacing(),
            align_member_colons: cfg.formatter_align_member_colons,
            annotation_placement: cfg.formatter_annotation_placement,
            default_placement: cfg.formatter_default_placement,
        };
        let Ok(path) = uri.to_file_path() else {
            return base;
        };
        let Some(dir) = path.parent() else {
            return base;
        };
        match format_config::load(dir) {
            Ok(Some(file)) => file.apply_to(base),
            Ok(None) => base,
            Err(error) => {
                warn!(error = %error, "ignoring malformed .wsformat.toml; using editor settings");
                base
            }
        }
    }
}
