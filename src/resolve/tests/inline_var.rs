use rstest::rstest;

use crate::resolve::extract_common::apply_splices;
use crate::resolve::inline_variable;
use crate::test_support::TestDb;

fn inlined(src: &str) -> Option<String> {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let byte = doc.line_index.position_to_byte(&doc.source, pos)?;
    let inlining = inline_variable(&uri, doc, &t.db(), byte)?;
    Some(apply_splices(&doc.source, &inlining.edits))
}

#[rstest]
#[case::all_usages_from_declaration(
    "all usages from declaration",
    "function f() {\n    var $0count : int = 5;\n    Foo(count);\n    Bar(count);\n}\n",
    "function f() {\n    Foo(5);\n    Bar(5);\n}\n"
)]
#[case::single_usage_from_use(
    "single usage from use",
    "function f() {\n    var count : int = 5;\n    Foo($0count);\n    Bar(count);\n}\n",
    "function f() {\n    var count : int = 5;\n    Foo(5);\n    Bar(count);\n}\n"
)]
#[case::wraps_compound_initializer(
    "wraps compound initializer",
    "function f() {\n    var $0sum : int = a + b;\n    return sum * 2;\n}\n",
    "function f() {\n    return (a + b) * 2;\n}\n"
)]
#[case::field_with_same_name_untouched(
    "field with same name untouched",
    "class C {\n    var count : int;\n    function f() {\n        var $0count : int = 5;\n        Foo(count);\n        Foo(this.count);\n    }\n}\n",
    "class C {\n    var count : int;\n    function f() {\n        Foo(5);\n        Foo(this.count);\n    }\n}\n"
)]
fn inlines(#[case] label: &str, #[case] src: &str, #[case] expected: &str) {
    let got = inlined(src).unwrap_or_else(|| panic!("case {label}: expected an inlining"));
    assert_eq!(got, expected, "case {label}: inlined output mismatch");
}

#[rstest]
#[case::no_initializer(
    "no initializer",
    "function f() {\n    var $0x : int;\n    Foo(x);\n}\n"
)]
#[case::multi_name_declaration(
    "multi-name declaration",
    "function f() {\n    var $0a, b : int = 0;\n    Foo(a);\n}\n"
)]
#[case::reassigned_variable(
    "reassigned variable",
    "function f() {\n    var $0x : int = 5;\n    x = 10;\n    Foo(x);\n}\n"
)]
#[case::single_usage_on_write_target(
    "single usage on write target",
    "function f() {\n    var x : int = 5;\n    $0x = 10;\n    Foo(x);\n}\n"
)]
fn refuses(#[case] label: &str, #[case] src: &str) {
    assert!(
        inlined(src).is_none(),
        "case {label}: expected no inlining offered"
    );
}
