use tree_sitter::Node;

use super::walk::{CstVisitor, Visit, walk};

pub(crate) fn collect_descendants_of_kind<'tree>(
    root: Node<'tree>,
    kinds: &[&str],
    out: &mut Vec<Node<'tree>>,
) {
    struct Collector<'a, 'tree> {
        kinds: &'a [&'a str],
        out: &'a mut Vec<Node<'tree>>,
    }
    impl<'tree> CstVisitor<'tree> for Collector<'_, 'tree> {
        fn enter(&mut self, node: Node<'tree>) -> Visit {
            if self.kinds.contains(&node.kind()) {
                self.out.push(node);
            }
            Visit::Children
        }
    }
    walk(root, &mut Collector { kinds, out });
}

pub(crate) fn has_descendant_of_kind(root: Node, kinds: &[&str]) -> bool {
    struct Finder<'a> {
        kinds: &'a [&'a str],
        found: bool,
    }
    impl<'tree> CstVisitor<'tree> for Finder<'_> {
        fn enter(&mut self, node: Node<'tree>) -> Visit {
            if self.kinds.contains(&node.kind()) {
                self.found = true;
                return Visit::SkipChildren;
            }
            Visit::Children
        }
    }
    let mut finder = Finder {
        kinds,
        found: false,
    };
    walk(root, &mut finder);
    finder.found
}
