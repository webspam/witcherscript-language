//! A request-scoped semantic model of one callable body, queried by the refactor code actions.
//!
//! The CST gives the syntax; this model adds the semantics the refactors keep re-deriving (which
//! identifier resolves to which local, which occurrences read a value). It is built fresh per
//! request from a parsed snapshot and never cached: an incremental edit invalidates it (invariant
//! 10). Consumers speak only in opaque handles and byte ranges, never tree-sitter nodes, so the
//! backing can change without touching a single caller.

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::descendants::collect_descendants_of_kind;
use crate::cst::grammar::write_target;
use crate::cst::nav::first_named_child;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::{SymbolId, SymbolKind};

use super::extract_common::CALLABLE_KINDS;
use super::name_context::{NameContext, classify_ident_context};

/// Opaque handle for a local or parameter of the modelled callable. Compare it and pass it back;
/// its internals are deliberately hidden so the identity scheme can change later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LocalId(SymbolId);

struct LocalEntry {
    id: SymbolId,
    selection: Range<usize>,
}

pub(crate) struct BodyModel {
    locals: Vec<LocalEntry>,
    reads_by_local: HashMap<SymbolId, Vec<Range<usize>>>,
}

impl BodyModel {
    /// Build the model for the callable body enclosing `byte`, or `None` outside any callable body.
    pub(crate) fn enclosing(document: &ParsedDocument, byte: usize) -> Option<Self> {
        let callable = document.symbols.enclosing_symbol_at(byte, CALLABLE_KINDS)?;
        let root = document.tree.root_node();
        let name = root.descendant_for_byte_range(
            callable.selection_byte_range.start,
            callable.selection_byte_range.end,
        )?;
        let decl = find_ancestor_of_kind(name, &[kinds::FUNC_DECL, kinds::EVENT_DECL])?;
        let body = decl
            .child_by_field_name(fields::DEFINITION)
            .filter(|n| n.kind() == kinds::FUNC_BLOCK)?;

        let locals = document
            .symbols
            .children_of(Some(callable.id))
            .filter(|s| matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter))
            .map(|s| LocalEntry {
                id: s.id,
                selection: s.selection_byte_range.clone(),
            })
            .collect();

        let reads_by_local = collect_reads(document, callable.id, body);
        Some(BodyModel {
            locals,
            reads_by_local,
        })
    }

    /// The local whose declaration name covers `byte`, or `None` if `byte` is not on a declaration.
    pub(crate) fn local_declared_at(&self, byte: usize) -> Option<LocalId> {
        self.locals
            .iter()
            .find(|e| e.selection.start <= byte && byte <= e.selection.end)
            .map(|e| LocalId(e.id))
    }

    /// Byte ranges of every occurrence that reads `local`'s value. A whole-value assignment target
    /// (`x = ...`) is not a read; a compound or path target (`x += ...`, `x.f = ...`) is.
    pub(crate) fn reads(&self, local: LocalId) -> &[Range<usize>] {
        self.reads_by_local.get(&local.0).map_or(&[], Vec::as_slice)
    }
}

fn collect_reads(
    document: &ParsedDocument,
    callable: SymbolId,
    body: Node,
) -> HashMap<SymbolId, Vec<Range<usize>>> {
    let source = &document.source;
    let bytes = source.as_bytes();
    let mut idents = Vec::new();
    collect_descendants_of_kind(body, &[kinds::IDENT], &mut idents);

    let mut reads: HashMap<SymbolId, Vec<Range<usize>>> = HashMap::new();
    for ident in idents {
        if classify_ident_context(ident, bytes) != Some(NameContext::Value) {
            continue;
        }
        if is_whole_value_write(ident) {
            continue;
        }
        let name = &source[ident.byte_range()];
        // Locals shadow fields and globals, so a value-context name match resolves to the local.
        if let Some(local) = document
            .symbols
            .local_at_byte(callable, name, ident.start_byte())
        {
            reads.entry(local.id).or_default().push(ident.byte_range());
        }
    }
    reads
}

// `x = ...` where `x` is the entire left-hand side: the prior value is overwritten, not read.
fn is_whole_value_write(ident: Node) -> bool {
    let Some(assign) = find_ancestor_of_kind(ident, &[kinds::ASSIGN_OP_EXPR]) else {
        return false;
    };
    let Some(left) = assign.child_by_field_name(fields::LEFT) else {
        return false;
    };
    if write_target(left).map(|n| n.id()) != Some(ident.id()) {
        return false;
    }
    let direct = assign
        .child_by_field_name(fields::OP)
        .is_some_and(|op| op.kind() == kinds::ASSIGN_OP_DIRECT);
    direct && unwrap_nested(left).id() == ident.id()
}

fn unwrap_nested(expr: Node) -> Node {
    match expr.kind() {
        kinds::NESTED_EXPR => first_named_child(expr).map_or(expr, unwrap_nested),
        _ => expr,
    }
}

#[cfg(test)]
mod tests;
