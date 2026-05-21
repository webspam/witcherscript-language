use tree_sitter::Node;

pub(crate) fn nodes_at_offset<'a>(root: Node<'a>, byte_offset: usize) -> Vec<Node<'a>> {
    let second = byte_offset.checked_sub(1);
    [Some(byte_offset), second]
        .into_iter()
        .flatten()
        .filter_map(|off| root.descendant_for_byte_range(off, off))
        .collect()
}

pub(crate) fn significant_node_before_byte<'a>(
    root: Node<'a>,
    source: &[u8],
    byte_offset: usize,
) -> Option<Node<'a>> {
    let mut end = byte_offset;
    loop {
        let p = source[..end]
            .iter()
            .rposition(|&b| !b.is_ascii_whitespace())?;
        let node = root.descendant_for_byte_range(p, p + 1)?;
        if node.kind() != "comment" {
            return Some(node);
        }
        end = node.start_byte();
    }
}

pub(crate) fn is_statement_boundary(node: Node) -> bool {
    if node.has_error() {
        return false;
    }
    if matches!(node.kind(), "{" | "}" | ";") {
        return true;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    // `)` closing an if condition without a curly-brace body is a statement boundary.
    let is_single_line_if = node.kind() == ")" && parent.kind() == "if_stmt";
    if is_single_line_if {
        return true;
    }
    if parent.kind() == "else_stmt" {
        return true;
    }
    // `:` closing a switch case/default label is also a statement boundary.
    node.kind() == ":" && matches!(parent.kind(), "switch_case_label" | "switch_default_label")
}

pub(crate) fn is_type_annotation_boundary(node: Node) -> bool {
    if node.has_error() {
        return false;
    }
    node.kind() == ":"
        && !node.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                "switch_case_label" | "switch_default_label" | "ternary_cond_expr"
            )
        })
}

pub(crate) fn is_kind_or_error_wrapped_kind(node: Node, kinds: &[&str]) -> bool {
    let effective = if node.is_error() && node.child_count() == 1 {
        node.child(0).unwrap()
    } else {
        node
    };
    kinds.contains(&effective.kind())
}

pub(crate) fn identifier_at(root: Node, byte_offset: usize) -> Option<Node> {
    nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|node| {
            if node.kind() == "ident" {
                return Some(node);
            }
            let mut current = node;
            while let Some(parent) = current.parent() {
                if parent.kind() == "ident" {
                    return Some(parent);
                }
                current = parent;
            }
            None
        })
}
