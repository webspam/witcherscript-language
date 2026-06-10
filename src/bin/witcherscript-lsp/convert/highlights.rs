use lsp_types::{DocumentHighlight, DocumentHighlightKind};
use witcherscript_language::line_index::SourceRange;
use witcherscript_language::resolve::HighlightKind;

use super::positions::lsp_range;

pub(crate) fn document_highlight(range: SourceRange, kind: HighlightKind) -> DocumentHighlight {
    let kind = match kind {
        HighlightKind::Read => DocumentHighlightKind::READ,
        HighlightKind::Write => DocumentHighlightKind::WRITE,
    };
    DocumentHighlight {
        range: lsp_range(range),
        kind: Some(kind),
    }
}
