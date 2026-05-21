use tree_sitter::Node;

pub(crate) fn node_and_ancestors<'tree>(node: Node<'tree>) -> impl Iterator<Item = Node<'tree>> {
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
