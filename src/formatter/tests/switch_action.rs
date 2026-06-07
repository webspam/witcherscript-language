use expect_test::expect;
use rstest::rstest;
use tree_sitter::Node;

use crate::document::{parse_document, ParsedDocument};
use crate::formatter::{
    analyze_switch, format_switch_with_layout, switch_stmt_on_keyword, FormatOptions, SwitchLayout,
};

fn first_switch(node: Node) -> Option<Node> {
    if node.kind() == "switch_stmt" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().find_map(first_switch)
}

fn switch_of(doc: &ParsedDocument) -> Node<'_> {
    first_switch(doc.tree.root_node()).expect("fixture has a switch")
}

fn apply(src: &str, layout: SwitchLayout) -> String {
    apply_with(src, layout, FormatOptions::default())
}

fn apply_with(src: &str, layout: SwitchLayout, options: FormatOptions) -> String {
    let doc = parse_document(src).expect("should parse");
    let switch_node = switch_of(&doc);
    let new_text = format_switch_with_layout(switch_node, &doc.source, options, layout);
    let mut out = doc.source.clone();
    out.replace_range(switch_node.start_byte()..switch_node.end_byte(), &new_text);
    out
}

const BLOCK_SWITCH: &str = include_str!("../../../tests/fixtures/formatter/switch_block.ws");
const INLINE_SWITCH: &str = include_str!("../../../tests/fixtures/formatter/switch_inline.ws");

#[rstest]
#[case::block_collapsible(
    "function F() {\nswitch (x) {\ncase 0:\nFoo();\nbreak;\ncase 1:\nBar();\nbreak;\n}\n}\n",
    true,
    false
)]
#[case::already_inline(
    "function F() {\nswitch (x) {\ncase 0: Foo(); break;\ncase 1: Bar(); break;\n}\n}\n",
    false,
    true
)]
#[case::mixed(
    "function F() {\nswitch (x) {\ncase 0: Foo(); break;\ncase 1:\nBar();\nbreak;\n}\n}\n",
    true,
    true
)]
#[case::two_non_break_blocks_collapse(
    "function F() {\nswitch (x) {\ncase 0:\nFoo();\nBar();\nbreak;\n}\n}\n",
    false,
    false
)]
#[case::multiline_body_blocks_collapse(
    "function F() {\nswitch (x) {\ncase 0:\nif (a) {\nFoo();\n}\nbreak;\n}\n}\n",
    false,
    false
)]
fn analyze_reports_legal_directions(
    #[case] src: &str,
    #[case] can_collapse: bool,
    #[case] can_expand: bool,
) {
    let doc = parse_document(src).expect("should parse");
    let toggle = analyze_switch(switch_of(&doc), &doc.source, FormatOptions::default());
    assert_eq!(toggle.can_collapse, can_collapse, "can_collapse mismatch");
    assert_eq!(toggle.can_expand, can_expand, "can_expand mismatch");
}

#[test]
fn collapse_past_line_limit_is_not_offered() {
    let src = "function F() {\n    switch (x) {\n        case 0:\n            SomeReasonablyLongCall();\n            break;\n    }\n}\n";
    let options = FormatOptions {
        line_limit: 30,
        ..Default::default()
    };
    let doc = parse_document(src).expect("should parse");
    let toggle = analyze_switch(switch_of(&doc), &doc.source, options);
    assert!(
        !toggle.can_collapse,
        "over-limit collapse must not be offered"
    );
}

#[test]
fn collapse_joins_each_case_onto_its_label() {
    expect![[r#"
        function F() {
            switch (x) {
                case 0:  Foo();  break;
                case 1:  Bar();  break;
            }
        }
    "#]]
    .assert_eq(&apply(BLOCK_SWITCH, SwitchLayout::Collapse));
}

#[test]
fn expand_puts_each_statement_on_its_own_line() {
    expect![[r#"
        function F() {
            switch (x) {
                case 0:
                    Foo();
                    break;
                case 1:
                    Bar();
                    break;
            }
        }
    "#]]
    .assert_eq(&apply(INLINE_SWITCH, SwitchLayout::Expand));
}

#[test]
fn collapse_output_is_stable_under_the_formatter() {
    let collapsed = apply(BLOCK_SWITCH, SwitchLayout::Collapse);
    let doc = parse_document(&collapsed).expect("should parse");
    let reformatted = crate::formatter::format_document(
        doc.tree.root_node(),
        &doc.source,
        FormatOptions::default(),
    );
    assert_eq!(reformatted, collapsed, "collapse must survive a reformat");
}

#[test]
fn expand_output_is_stable_under_the_formatter() {
    let expanded = apply(INLINE_SWITCH, SwitchLayout::Expand);
    let doc = parse_document(&expanded).expect("should parse");
    let reformatted = crate::formatter::format_document(
        doc.tree.root_node(),
        &doc.source,
        FormatOptions::default(),
    );
    assert_eq!(reformatted, expanded, "expand must survive a reformat");
}

#[rstest]
#[case::switch_kw("switch", true)]
#[case::case_kw("case", true)]
#[case::default_kw("default", true)]
#[case::statement("Foo", false)]
fn keyword_trigger_finds_the_switch(#[case] needle: &str, #[case] expected: bool) {
    let src = "function F() {\n    switch (x) {\n        case 0:\n            Foo();\n            break;\n        default:\n            break;\n    }\n}\n";
    let doc = parse_document(src).expect("should parse");
    let byte = src.find(needle).expect("needle present") + 1;
    let found = switch_stmt_on_keyword(doc.tree.root_node(), byte).is_some();
    assert_eq!(found, expected, "keyword {needle}");
}
