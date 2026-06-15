use tree_sitter::Node;

use super::kinds;

/// A single literal token. The sign is part of a numeric token, so `-13` counts; computed or
/// referencing values (`a + b`, calls, idents) do not, even when constant-foldable.
pub(crate) fn is_constant_literal(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        kinds::LITERAL_INT
            | kinds::LITERAL_HEX
            | kinds::LITERAL_FLOAT
            | kinds::LITERAL_STRING
            | kinds::LITERAL_NAME
            | kinds::LITERAL_BOOL
            | kinds::LITERAL_NULL
    )
}

#[cfg(test)]
mod tests;
