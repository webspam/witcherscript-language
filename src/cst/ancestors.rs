use tree_sitter::Node;

use super::kinds;

pub(crate) fn node_and_ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    let mut next = Some(node);
    std::iter::from_fn(move || {
        let current = next?;
        next = current.parent();
        Some(current)
    })
}

pub(crate) fn find_ancestor_of_kind<'tree>(
    node: Node<'tree>,
    kinds: &[&str],
) -> Option<Node<'tree>> {
    node_and_ancestors(node).find(|n| kinds.contains(&n.kind()))
}

pub(crate) fn has_ancestor_of_kind(node: Node, kinds: &[&str]) -> bool {
    find_ancestor_of_kind(node, kinds).is_some()
}

pub(crate) fn enclosing_callable_block(node: Node) -> Option<Node> {
    node_and_ancestors(node).find(|n| {
        n.kind() == kinds::FUNC_BLOCK
            && n.parent()
                .is_some_and(|p| matches!(p.kind(), kinds::FUNC_DECL | kinds::EVENT_DECL))
    })
}
