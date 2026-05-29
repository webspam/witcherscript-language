use crate::cst::offsets::offset_in_comment;
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;

pub fn position_in_comment(document: &ParsedDocument, position: SourcePosition) -> bool {
    let Some(byte_offset) = document
        .line_index
        .position_to_byte(&document.source, position)
    else {
        return false;
    };
    offset_in_comment(document.tree.root_node(), byte_offset)
}
