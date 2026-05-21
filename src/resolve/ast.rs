use tree_sitter::Node;

use crate::symbols::SymbolKind;

pub(super) use crate::cst::nav::first_named_child;
pub(super) use crate::cst::offsets::{
    identifier_at, is_kind_or_error_wrapped_kind, is_statement_boundary,
    is_type_annotation_boundary, nodes_at_offset, significant_node_before_byte,
};

pub const BUILTIN_TYPES: &[&str] = &[
    "bool",
    "byte",
    "float",
    "int",
    "name",
    "string",
    "void",
    "Bool",
    "Float",
    "String",
    "CName",
    "Int32",
    "Int16",
    "Int8",
    "Uint8",
    "Uint16",
    "Uint32",
    "Uint64",
    "StringAnsi",
];

pub const BUILTIN_TYPE_COMPLETIONS: &[&str] =
    &["bool", "byte", "float", "int", "name", "string", "void"];

pub(super) fn find_ancestor_of_kind<'a>(mut node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    loop {
        if kinds.contains(&node.kind()) {
            return Some(node);
        }
        node = node.parent()?;
    }
}

pub(super) fn nearest_enclosing_block<'a>(mut node: Node<'a>) -> Option<Node<'a>> {
    const BLOCKS: &[&str] = &["func_block", "switch_block", "member_default_val_block"];
    loop {
        if BLOCKS.contains(&node.kind()) {
            return Some(node);
        }
        node = node.parent()?;
    }
}

pub(super) fn is_type_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
    )
}
