use lsp_types::{Position, Range};
use witcherscript_language::line_index::{SourcePosition, SourceRange};

pub(crate) fn lsp_range(range: SourceRange) -> Range {
    Range {
        start: Position {
            line: range.start.line,
            character: range.start.character,
        },
        end: Position {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

pub(crate) fn source_range(start: SourcePosition, end: SourcePosition) -> SourceRange {
    SourceRange { start, end }
}

pub(crate) fn source_position(position: Position) -> SourcePosition {
    SourcePosition {
        line: position.line,
        character: position.character,
    }
}
