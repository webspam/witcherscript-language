use tree_sitter::Node;

use crate::cst::grammar::arg_slots;
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::line_index::{SourcePosition, SourceRange};
use crate::symbols::node_text;

use super::definition::callee_params;
use super::symbol_db::SymbolDb;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHintInfo {
    pub position: SourcePosition,
    pub label: String,
}

/// Parameter-name hints for every resolvable call whose span intersects `range`.
/// Returns `None` if cancelled mid-walk (the caller maps that to `CONTENT_MODIFIED`).
pub fn inlay_hints(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    range: SourceRange,
    should_continue: &dyn Fn() -> bool,
) -> Option<Vec<InlayHintInfo>> {
    let lo = document
        .line_index
        .position_to_byte(&document.source, range.start)
        .unwrap_or(0);
    let hi = document
        .line_index
        .position_to_byte(&document.source, range.end)
        .unwrap_or(document.source.len());

    let ctx = Walk {
        uri,
        document,
        db,
        should_continue,
        lo,
        hi,
    };
    let mut hints = Vec::new();
    ctx.visit(document.tree.root_node(), &mut hints)
        .then_some(hints)
}

struct Walk<'a, 'db> {
    uri: &'a str,
    document: &'a ParsedDocument,
    db: &'a SymbolDb<'db>,
    should_continue: &'a dyn Fn() -> bool,
    lo: usize,
    hi: usize,
}

impl Walk<'_, '_> {
    /// Returns `false` if the walk was cancelled.
    fn visit(&self, node: Node, hints: &mut Vec<InlayHintInfo>) -> bool {
        if node.end_byte() <= self.lo || node.start_byte() >= self.hi {
            return true;
        }
        if node.kind() == kinds::FUNC_CALL_EXPR {
            if !(self.should_continue)() {
                return false;
            }
            self.emit_call(node, hints);
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if !self.visit(child, hints) {
                return false;
            }
        }
        true
    }

    fn emit_call(&self, call: Node, hints: &mut Vec<InlayHintInfo>) {
        let Some(slots) = arg_slots(call) else {
            return;
        };
        let Some(params) = callee_params(self.uri, self.document, self.db, call) else {
            return;
        };
        for (param, arg) in params.iter().zip(slots.iter()) {
            // Suppress a redundant name echo, except for `out` params whose write-through is the point.
            if !param.is_out
                && arg.kind() == kinds::IDENT
                && node_text(*arg, &self.document.source) == param.name
            {
                continue;
            }
            let position = self
                .document
                .line_index
                .byte_to_position(&self.document.source, arg.start_byte());
            let label = if param.is_out {
                format!("out {}:", param.name)
            } else {
                format!("{}:", param.name)
            };
            hints.push(InlayHintInfo { position, label });
        }
    }
}
