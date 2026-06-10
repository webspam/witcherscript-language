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

/// One traversal, two visitors: each sees exactly its solo event stream
/// (events below its skip are masked); the walk prunes only when both skip.
pub(crate) struct Fused<'v, A, B> {
    a: &'v mut A,
    b: &'v mut B,
    a_skip: Option<usize>,
    b_skip: Option<usize>,
    depth: usize,
}

impl<'v, A, B> Fused<'v, A, B> {
    pub(crate) fn new(a: &'v mut A, b: &'v mut B) -> Self {
        Self {
            a,
            b,
            a_skip: None,
            b_skip: None,
            depth: 0,
        }
    }
}

impl<'tree, A: CstVisitor<'tree>, B: CstVisitor<'tree>> CstVisitor<'tree> for Fused<'_, A, B> {
    fn enter(&mut self, node: Node<'tree>) -> Visit {
        if self.a_skip.is_none() && self.a.enter(node) == Visit::SkipChildren {
            self.a_skip = Some(self.depth);
        }
        if self.b_skip.is_none() && self.b.enter(node) == Visit::SkipChildren {
            self.b_skip = Some(self.depth);
        }
        self.depth += 1;
        if self.a_skip.is_some() && self.b_skip.is_some() {
            Visit::SkipChildren
        } else {
            Visit::Children
        }
    }

    fn leave(&mut self, node: Node<'tree>) {
        self.depth -= 1;
        match self.a_skip {
            Some(depth) if depth == self.depth => {
                self.a_skip = None;
                self.a.leave(node);
            }
            Some(_) => {}
            None => self.a.leave(node),
        }
        match self.b_skip {
            Some(depth) if depth == self.depth => {
                self.b_skip = None;
                self.b.leave(node);
            }
            Some(_) => {}
            None => self.b.leave(node),
        }
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter::Node;

    use super::{CstVisitor, Fused, Visit, walk};
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
    fn fused_visitors_each_see_their_solo_event_stream() {
        let doc = parse_document(FIXTURE).expect("parse");
        let root = doc.tree.root_node();

        let configs = [
            (None, Some(kinds::FUNC_BLOCK)),
            (Some(kinds::FUNC_DECL), None),
            (Some(kinds::FUNC_BLOCK), Some(kinds::FUNC_BLOCK)),
        ];
        for (skip_a, skip_b) in configs {
            let mut solo_a = Recorder::new(skip_a);
            walk(root, &mut solo_a);
            let mut solo_b = Recorder::new(skip_b);
            walk(root, &mut solo_b);

            let mut fused_a = Recorder::new(skip_a);
            let mut fused_b = Recorder::new(skip_b);
            walk(root, &mut Fused::new(&mut fused_a, &mut fused_b));

            let label = format!("skip_a={skip_a:?} skip_b={skip_b:?}");
            assert_eq!(fused_a.enters, solo_a.enters, "{label}: a enters diverge");
            assert_eq!(fused_a.leaves, solo_a.leaves, "{label}: a leaves diverge");
            assert_eq!(fused_b.enters, solo_b.enters, "{label}: b enters diverge");
            assert_eq!(fused_b.leaves, solo_b.leaves, "{label}: b leaves diverge");
        }
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
