use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Visit {
    Children,
    SkipChildren,
}

pub(crate) trait CstVisitor<'tree> {
    fn enter(&mut self, node: Node<'tree>) -> Visit;
    fn leave(&mut self, _node: Node<'tree>) {}
}

/// Pre-order `enter`, post-order `leave`. Every delivered `enter` gets exactly one
/// matching `leave`, including for a node whose `enter` returned `SkipChildren`;
/// that pairing is what lets visitors keep stack state.
pub(crate) fn walk<'tree, V: CstVisitor<'tree>>(root: Node<'tree>, visitor: &mut V) {
    // One iterative cursor: no per-node cursor allocation, no stack-overflow risk
    // on deeply nested error-recovery trees. A cursor cannot escape `root`'s subtree.
    let mut cursor = root.walk();
    loop {
        if visitor.enter(cursor.node()) == Visit::Children && cursor.goto_first_child() {
            continue;
        }
        loop {
            visitor.leave(cursor.node());
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter::Node;

    use super::{CstVisitor, Visit, walk};
    use crate::cst::kinds;
    use crate::document::parse_document;

    struct Recorder {
        skip_kind: Option<&'static str>,
        enters: Vec<(usize, String)>,
        leaves: Vec<(usize, String)>,
        open: Vec<usize>,
    }

    impl Recorder {
        fn new(skip_kind: Option<&'static str>) -> Self {
            Self {
                skip_kind,
                enters: Vec::new(),
                leaves: Vec::new(),
                open: Vec::new(),
            }
        }
    }

    impl<'tree> CstVisitor<'tree> for Recorder {
        fn enter(&mut self, node: Node<'tree>) -> Visit {
            self.enters.push((node.id(), node.kind().to_string()));
            self.open.push(node.id());
            if self.skip_kind == Some(node.kind()) {
                Visit::SkipChildren
            } else {
                Visit::Children
            }
        }

        fn leave(&mut self, node: Node<'tree>) {
            let opened = self.open.pop();
            assert_eq!(
                opened,
                Some(node.id()),
                "leave must close the most recently entered node"
            );
            self.leaves.push((node.id(), node.kind().to_string()));
        }
    }

    fn recursive_preorder(node: Node, out: &mut Vec<usize>) {
        out.push(node.id());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            recursive_preorder(child, out);
        }
    }

    const FIXTURE: &str = "\
// leading comment
function First() {
    var x : int;
    x = ;
}

function Second() {
    return 1;
}
";

    #[test]
    fn enter_order_matches_recursive_preorder() {
        let doc = parse_document(FIXTURE).expect("parse");
        let root = doc.tree.root_node();
        assert!(root.has_error(), "fixture must exercise error recovery");

        let mut recorder = Recorder::new(None);
        walk(root, &mut recorder);

        let mut expected = Vec::new();
        recursive_preorder(root, &mut expected);
        let entered: Vec<usize> = recorder.enters.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            entered, expected,
            "enter order must equal recursive pre-order"
        );
    }

    #[test]
    fn every_enter_gets_exactly_one_leave() {
        let doc = parse_document(FIXTURE).expect("parse");

        for skip in [None, Some(kinds::FUNC_BLOCK)] {
            let mut recorder = Recorder::new(skip);
            walk(doc.tree.root_node(), &mut recorder);
            assert!(recorder.open.is_empty(), "all entered nodes must be left");
            let mut enters = recorder.enters.clone();
            let mut leaves = recorder.leaves.clone();
            enters.sort_unstable();
            leaves.sort_unstable();
            assert_eq!(
                enters, leaves,
                "skip={skip:?}: enters and leaves must pair 1:1"
            );
        }
    }

    #[test]
    fn skip_children_prunes_subtree() {
        let doc = parse_document(FIXTURE).expect("parse");

        let mut recorder = Recorder::new(Some(kinds::FUNC_BLOCK));
        walk(doc.tree.root_node(), &mut recorder);

        let entered_kinds: Vec<&str> = recorder.enters.iter().map(|(_, k)| k.as_str()).collect();
        assert!(
            entered_kinds.contains(&kinds::FUNC_BLOCK),
            "the skipped node itself is still entered"
        );
        assert!(
            !entered_kinds.contains(&kinds::LOCAL_VAR_DECL_STMT),
            "descendants of a skipped node must not be entered"
        );
        assert!(
            recorder.leaves.iter().any(|(_, k)| k == kinds::FUNC_BLOCK),
            "a skipped node still gets its leave"
        );
    }

    #[test]
    fn walking_a_child_stays_inside_its_subtree() {
        let doc = parse_document(FIXTURE).expect("parse");
        let root = doc.tree.root_node();
        let mut cursor = root.walk();
        let first = root
            .children(&mut cursor)
            .find(|n| n.kind() == kinds::FUNC_DECL)
            .expect("fixture has a function");

        let mut recorder = Recorder::new(None);
        walk(first, &mut recorder);

        let mut expected = Vec::new();
        recursive_preorder(first, &mut expected);
        let entered: Vec<usize> = recorder.enters.iter().map(|(id, _)| *id).collect();
        assert_eq!(entered, expected, "walk must cover exactly the subtree");
    }
}
