use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position};
use witcherscript_language::resolve::InlayHintInfo;

pub(crate) fn inlay_hint(info: InlayHintInfo) -> InlayHint {
    InlayHint {
        position: Position {
            line: info.position.line,
            character: info.position.character,
        },
        label: InlayHintLabel::String(info.label),
        kind: Some(InlayHintKind::PARAMETER),
        text_edits: None,
        tooltip: None,
        padding_left: None,
        padding_right: Some(true),
        data: None,
    }
}
