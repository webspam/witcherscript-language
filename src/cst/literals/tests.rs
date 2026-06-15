use rstest::rstest;
use tree_sitter::Node;

use super::is_constant_literal;
use crate::cst::{fields, kinds};
use crate::document::parse_document;

fn find_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| find_kind(child, kind))
}

fn initialiser_is_constant(value_src: &str) -> bool {
    let src = format!("function F() {{ var x : int = {value_src}; }}\n");
    let doc = parse_document(src).expect("fixture parses");
    let init = find_kind(doc.tree.root_node(), kinds::LOCAL_VAR_DECL_STMT)
        .and_then(|stmt| stmt.child_by_field_name(fields::INIT_VALUE))
        .expect("initialiser present");
    is_constant_literal(init)
}

#[rstest]
#[case::int("13", true)]
#[case::hex("0xFF", true)]
#[case::signed_int("-13", true)]
#[case::float("1.5", true)]
#[case::string("\"text\"", true)]
#[case::cname("'aName'", true)]
#[case::boolean("true", true)]
#[case::null("NULL", true)]
#[case::concatenation("\"a\" + \"b\"", false)]
#[case::reference("foo", false)]
#[case::call("Bar()", false)]
#[case::cast("(int) 1", false)]
fn classifies_constant_literals(#[case] value: &str, #[case] expected: bool) {
    assert_eq!(
        initialiser_is_constant(value),
        expected,
        "value {value:?}: is_constant_literal",
    );
}
