use expect_test::expect;
use rstest::rstest;
use tree_sitter::Node;

use crate::document::{ParsedDocument, parse_document};
use crate::formatter::{FormatOptions, IfLayout, analyze_if, if_chain_at, rewrite_if_layout};

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
    let new_text = rewrite_if_layout(if_node, &doc.source, FormatOptions::default(), layout);
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
    let toggle = analyze_if(if_of(&doc), FormatOptions::default());
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
    let toggle = analyze_if(if_of(&doc), options);
    assert!(
        !toggle.can_collapse,
        "over-limit collapse must not be offered"
    );
}

#[test]
fn hand_broken_condition_is_not_collapsible() {
    let src = "function F() {\n    if (a &&\n        b) {\n        Foo();\n    }\n}\n";
    let doc = parse_document(src).expect("should parse");
    let toggle = analyze_if(if_of(&doc), FormatOptions::default());
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

#[test]
fn collapse_keeps_body_spacing_verbatim() {
    let src = "function F() {\n    if (cond) {\n        Do( p,q );\n    }\n}\n";
    let got = apply(src, IfLayout::Collapse);
    assert!(
        got.contains("if (cond) Do( p,q );"),
        "collapse must move the body verbatim, not reformat it; got:\n{got}"
    );
}

#[test]
fn expand_keeps_body_spacing_verbatim() {
    let src = "function F() {\n    if (cond) Do( p,q );\n}\n";
    let got = apply(src, IfLayout::Expand);
    assert!(
        got.contains("Do( p,q );"),
        "expand must wrap the body verbatim, not reformat it; got:\n{got}"
    );
}

#[test]
fn expand_indents_from_actual_whitespace() {
    let src = "function F() {\n   if (cond) Do();\n}\n";
    let got = apply(src, IfLayout::Expand);
    assert_eq!(
        got, "function F() {\n   if (cond) {\n       Do();\n   }\n}\n",
        "expand must indent from the line's actual whitespace"
    );
}

#[rstest]
#[case::head_if("if (a)")]
#[case::else_if("else if")]
#[case::trailing_else_body("Baz")]
fn cursor_in_chain_resolves_to_head(#[case] needle: &str) {
    let src = "function F() {\n    if (a) Foo();\n    else if (b) Bar();\n    else Baz();\n}\n";
    let doc = parse_document(src).expect("should parse");
    let head = src.find("if (a)").expect("head present");
    let byte = src.find(needle).expect("needle present") + 1;
    let found = if_chain_at(doc.tree.root_node(), byte).expect("cursor is inside a chain");
    assert_eq!(
        found.start_byte(),
        head,
        "case {needle}: must climb to the chain head"
    );
}

#[test]
fn cursor_outside_any_chain_is_none() {
    let src = "function F() {\n    if (a) Foo();\n}\n";
    let doc = parse_document(src).expect("should parse");
    let byte = src.find("function").expect("needle present") + 1;
    assert!(if_chain_at(doc.tree.root_node(), byte).is_none());
}
