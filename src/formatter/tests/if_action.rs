use expect_test::expect;
use rstest::rstest;
use tree_sitter::Node;

use crate::document::{parse_document, ParsedDocument};
use crate::formatter::{
    analyze_if, format_if_with_layout, if_stmt_on_keyword, FormatOptions, IfLayout,
};

fn first_if(node: Node) -> Option<Node> {
    if node.kind() == "if_stmt" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().find_map(first_if)
}

fn if_of(doc: &ParsedDocument) -> Node<'_> {
    first_if(doc.tree.root_node()).expect("fixture has an if")
}

fn apply(src: &str, layout: IfLayout) -> String {
    let doc = parse_document(src).expect("should parse");
    let if_node = if_of(&doc);
    let new_text = format_if_with_layout(if_node, &doc.source, FormatOptions::default(), layout);
    let mut out = doc.source.clone();
    out.replace_range(if_node.start_byte()..if_node.end_byte(), &new_text);
    out
}

#[rstest]
#[case::block_collapsible(
    "function F() {\nif (a) {\nFoo();\n}\nelse {\nBar();\n}\n}\n",
    true,
    false
)]
#[case::already_inline("function F() {\nif (a) Foo();\nelse Bar();\n}\n", false, true)]
#[case::single_block("function F() {\nif (a) {\nFoo();\n}\n}\n", true, false)]
#[case::single_inline("function F() {\nif (a) Foo();\n}\n", false, true)]
#[case::mixed("function F() {\nif (a) Foo();\nelse {\nBar();\n}\n}\n", true, true)]
#[case::two_stmt_block("function F() {\nif (a) {\nFoo();\nBar();\n}\n}\n", false, false)]
#[case::compound_body_block(
    "function F() {\nif (a) {\nwhile (b) {\nFoo();\n}\n}\n}\n",
    false,
    false
)]
#[case::dangling_else_block(
    "function F() {\nif (a) {\nif (b) Foo();\n}\nelse Bar();\n}\n",
    false,
    true
)]
fn analyze_reports_legal_directions(
    #[case] src: &str,
    #[case] can_collapse: bool,
    #[case] can_expand: bool,
) {
    let doc = parse_document(src).expect("should parse");
    let toggle = analyze_if(if_of(&doc), &doc.source, FormatOptions::default());
    assert_eq!(toggle.can_collapse, can_collapse, "can_collapse mismatch");
    assert_eq!(toggle.can_expand, can_expand, "can_expand mismatch");
}

#[test]
fn collapse_past_line_limit_is_not_offered() {
    let src = "function F() {\n    if (x) {\n        SomeReasonablyLongCall();\n    }\n}\n";
    let options = FormatOptions {
        line_limit: 30,
        ..Default::default()
    };
    let doc = parse_document(src).expect("should parse");
    let toggle = analyze_if(if_of(&doc), &doc.source, options);
    assert!(
        !toggle.can_collapse,
        "over-limit collapse must not be offered"
    );
}

#[test]
fn hand_broken_condition_is_not_collapsible() {
    let src = "function F() {\n    if (a &&\n        b) {\n        Foo();\n    }\n}\n";
    let doc = parse_document(src).expect("should parse");
    let toggle = analyze_if(if_of(&doc), &doc.source, FormatOptions::default());
    assert!(
        !toggle.can_collapse,
        "a split condition would re-expand, so collapse must not be offered"
    );
}

#[test]
fn collapse_joins_each_branch_onto_one_line() {
    expect![[r#"
        function F() {
            if (a) Foo();
            else if (b) Bar();
            else Baz();
        }
    "#]]
    .assert_eq(&apply(
        include_str!("../../../tests/fixtures/formatter/if_block.ws"),
        IfLayout::Collapse,
    ));
}

#[test]
fn expand_gives_each_branch_a_block_body() {
    expect![[r#"
        function F() {
            if (a) {
                Foo();
            }
            else if (b) {
                Bar();
            }
            else {
                Baz();
            }
        }
    "#]]
    .assert_eq(&apply(
        include_str!("../../../tests/fixtures/formatter/if_inline.ws"),
        IfLayout::Expand,
    ));
}

#[test]
fn expand_leaves_a_nested_if_untouched() {
    expect![[r#"
        function F() {
            if (a) {
                Foo();
            }
            else {
                if (b) Bar();
            }
        }
    "#]]
    .assert_eq(&apply(
        include_str!("../../../tests/fixtures/formatter/if_nested.ws"),
        IfLayout::Expand,
    ));
}

#[rstest]
#[case::collapse(
    include_str!("../../../tests/fixtures/formatter/if_block.ws"),
    IfLayout::Collapse
)]
#[case::expand(
    include_str!("../../../tests/fixtures/formatter/if_inline.ws"),
    IfLayout::Expand
)]
fn rewrite_output_is_stable_under_the_formatter(#[case] src: &str, #[case] layout: IfLayout) {
    let rewritten = apply(src, layout);
    let doc = parse_document(&rewritten).expect("should parse");
    let reformatted = crate::formatter::format_document(
        doc.tree.root_node(),
        &doc.source,
        FormatOptions::default(),
    );
    assert_eq!(reformatted, rewritten, "rewrite must survive a reformat");
}

#[rstest]
#[case::if_kw("if", true)]
#[case::else_kw("else", true)]
#[case::statement("Foo", false)]
fn keyword_trigger_finds_the_chain(#[case] needle: &str, #[case] expected: bool) {
    let src =
        "function F() {\n    if (a) {\n        Foo();\n    }\n    else {\n        Bar();\n    }\n}\n";
    let doc = parse_document(src).expect("should parse");
    let byte = src.find(needle).expect("needle present") + 1;
    let found = if_stmt_on_keyword(doc.tree.root_node(), byte).is_some();
    assert_eq!(found, expected, "keyword {needle}");
}
