use tree_sitter::Node;

pub(super) use crate::cst::ancestors::find_ancestor_of_kind;
pub(super) use crate::cst::nav::first_named_child;
pub(super) use crate::cst::offsets::{
    identifier_at, is_kind_or_error_wrapped_kind, is_statement_boundary,
    is_type_annotation_boundary, nodes_at_offset, significant_node_before_byte,
};

pub const BUILTIN_TYPE_COMPLETIONS: &[&str] =
    &["bool", "byte", "float", "int", "name", "string", "void"];

pub(super) fn nearest_enclosing_block(node: Node) -> Option<Node> {
    const BLOCKS: &[&str] = &["func_block", "switch_block", "member_default_val_block"];
    find_ancestor_of_kind(node, BLOCKS)
}
