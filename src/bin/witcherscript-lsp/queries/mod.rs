use lsp_types::{Position, Url};
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
    fn format_options(&self, use_tabs: bool, tab_size: u32) -> FormatOptions {
        let cfg = self.config.load();
        FormatOptions {
            tab_size,
            use_tabs,
            line_limit: cfg.formatter_line_limit,
            colon: cfg.colon_spacing(),
            align_member_colons: cfg.formatter_align_member_colons,
            annotation_placement: cfg.formatter_annotation_placement,
            default_placement: cfg.formatter_default_placement,
        }
    }
}
