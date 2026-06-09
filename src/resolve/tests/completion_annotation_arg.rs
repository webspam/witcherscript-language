use std::fmt::Write;

use expect_test::expect;
use rstest::rstest;

use super::super::annotation_arg_completions;
use crate::test_support::TestDb;

// Empty parens are an error-recovery shape (the grammar requires `'(' ident ')'`).
// Our completion routing depends on it, so lock the tree a grammar bump would change.
#[test]
fn empty_annotation_parens_parse_shape() {
    fn dump(node: tree_sitter::Node, out: &mut String, depth: usize) {
        out.push_str(&"  ".repeat(depth));
        writeln!(
            out,
            "{} [{}..{}]",
            node.kind(),
            node.start_byte(),
            node.end_byte()
        )
        .unwrap();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            dump(child, out, depth + 1);
        }
    }

    let t = TestDb::new("@wrapMethod()\n");
    let mut out = String::new();
    dump(t.primary_doc().tree.root_node(), &mut out, 0);
    expect![[r"
        script [0..14]
          ERROR [0..13]
            annotation_ident [0..11]
            ( [11..12]
            ) [12..13]
    "]]
    .assert_eq(&out);
}

#[rstest]
#[case::add_field(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@addField($0CPlayer)\n",
    true
)]
#[case::add_method(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@addMethod($0CPlayer)\n",
    true
)]
#[case::wrap_method(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@wrapMethod($0CPlayer)\n",
    true
)]
#[case::empty_parens(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@wrapMethod($0)\n",
    true
)]
#[case::replace_method(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@replaceMethod($0CPlayer)\n",
    true
)]
#[case::unknown_annotation(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@someUnknownAnnotation($0CPlayer)\n",
    false
)]
fn annotation_arg_completions_offers_classes(#[case] fixture: &str, #[case] should_fire: bool) {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    let completions = annotation_arg_completions(t.primary_doc(), &t.db(), pos);

    let names: Vec<&str> = completions.iter().map(|d| d.symbol.name.as_str()).collect();
    if should_fire {
        assert!(
            names.contains(&"CPlayer"),
            "class should be offered inside parens"
        );
        assert!(
            !names.contains(&"SData"),
            "struct should not be offered inside parens"
        );
        assert!(
            !names.contains(&"EDir"),
            "enum should not be offered inside parens"
        );
    } else {
        assert!(
            completions.is_empty(),
            "unknown annotation must not get class completion"
        );
    }
}

#[test]
fn annotation_arg_completions_empty_outside_annotation() {
    let t = TestDb::new(concat!("class CPlayer {}\n", "function $0Test() {}\n",));
    let (_uri, pos) = t.cursor();
    let completions = annotation_arg_completions(t.primary_doc(), &t.db(), pos);

    assert!(
        completions.is_empty(),
        "annotation_arg_completions must not fire outside an annotation"
    );
}

#[test]
fn annotation_arg_completions_empty_after_closing_paren() {
    let t = TestDb::new("@wrapMethod(CPlayer) $0\n");
    let (_uri, pos) = t.cursor();
    let completions = annotation_arg_completions(t.primary_doc(), &t.db(), pos);

    assert!(
        completions.is_empty(),
        "annotation_arg_completions must not fire after the closing paren"
    );
}
