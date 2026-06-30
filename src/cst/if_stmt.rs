use tree_sitter::Node;

use super::ancestors::node_and_ancestors;
use super::{fields, kinds};

#[derive(PartialEq)]
enum IfBranch {
    Body,
    Else,
}

pub(crate) fn mutually_exclusive_branches(a: Node, b: Node) -> bool {
    node_and_ancestors(a)
        .filter(|n| n.kind() == kinds::IF_STMT)
        .any(
            |if_stmt| match (if_branch_of(if_stmt, a), if_branch_of(if_stmt, b)) {
                (Some(branch_a), Some(branch_b)) => branch_a != branch_b,
                _ => false,
            },
        )
}

// Chain head (outermost `if`) above an else-branch statement and the conditions leading to it; None
// when the statement is not in an `else if` position.
pub(crate) fn if_chain_above(statement: Node) -> Option<(Node, Vec<Node>)> {
    let mut conditions = Vec::new();
    let mut head = statement;
    while let Some(parent) = head.parent() {
        if parent.kind() != kinds::IF_STMT {
            break;
        }
        let is_else = parent
            .child_by_field_name(fields::ELSE)
            .is_some_and(|e| e.id() == head.id());
        if !is_else {
            break;
        }
        if let Some(cond) = parent.child_by_field_name(fields::COND) {
            conditions.push(cond);
        }
        head = parent;
    }
    (!conditions.is_empty()).then_some((head, conditions))
}

fn if_branch_of(if_stmt: Node, node: Node) -> Option<IfBranch> {
    let within = |field| {
        if_stmt
            .child_by_field_name(field)
            .is_some_and(|c| c.start_byte() <= node.start_byte() && node.end_byte() <= c.end_byte())
    };
    if within(fields::BODY) {
        Some(IfBranch::Body)
    } else if within(fields::ELSE) {
        Some(IfBranch::Else)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tree_sitter::Node;

    use super::mutually_exclusive_branches;
    use crate::document::parse_document;

    const SRC: &str = "\
function F() {
  if (c) {
    aBody;
    aBody2;
  } else {
    bElse;
  }
  cAfter;
  if (d) {
    if (e) {
      xInner;
    } else {
      yInner;
    }
    zOuter;
  }
}
";

    fn node_at<'t>(root: Node<'t>, needle: &str) -> Node<'t> {
        let off = SRC.find(needle).expect("needle present");
        root.descendant_for_byte_range(off, off + 1)
            .expect("node at needle")
    }

    #[rstest]
    #[case::if_versus_else("aBody", "bElse", true)]
    #[case::same_branch("aBody", "aBody2", false)]
    #[case::no_shared_if("aBody", "cAfter", false)]
    #[case::nested_if_else("xInner", "yInner", true)]
    #[case::outer_body_versus_inner_else("zOuter", "yInner", false)]
    #[case::separate_top_level_ifs("aBody", "xInner", false)]
    fn detects_mutually_exclusive_branches(
        #[case] a: &str,
        #[case] b: &str,
        #[case] expected: bool,
    ) {
        let doc = parse_document(SRC).expect("fixture parses");
        let root = doc.tree.root_node();
        assert_eq!(
            mutually_exclusive_branches(node_at(root, a), node_at(root, b)),
            expected,
            "{a} vs {b}"
        );
    }
}
