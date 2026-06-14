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
#[case::all_usages_from_name_end(
    "all usages from end of declaration name",
    "function f() {\n    var count$0 : int = 5;\n    Foo(count);\n    Bar(count);\n}\n",
    "function f() {\n    Foo(5);\n    Bar(5);\n}\n"
)]
#[case::all_usages_from_var_keyword(
    "all usages from var keyword",
    "function f() {\n    va$0r count : int = 5;\n    Foo(count);\n    Bar(count);\n}\n",
    "function f() {\n    Foo(5);\n    Bar(5);\n}\n"
)]
#[case::all_usages_before_var_keyword(
    "all usages before var keyword",
    "function f() {\n    $0var count : int = 5;\n    Foo(count);\n    Bar(count);\n}\n",
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
#[case::last_use_deletes_declaration(
    "last use deletes declaration",
    "function f() {\n    var count : int = 5;\n    return $0count;\n}\n",
    "function f() {\n    return 5;\n}\n"
)]
#[case::assign_later_from_use(
    "assign later, inline from use",
    "function f() {\n    var x : int;\n    x = 13;\n    return $0x;\n}\n",
    "function f() {\n    return 13;\n}\n"
)]
#[case::assign_later_from_declaration(
    "assign later, inline from declaration",
    "function f() {\n    var $0x : int;\n    x = 13;\n    return x;\n}\n",
    "function f() {\n    return 13;\n}\n"
)]
#[case::assign_later_all_usages(
    "assign later, multiple usages",
    "function f() {\n    var $0x : int;\n    x = 13;\n    Foo(x);\n    Bar(x);\n}\n",
    "function f() {\n    Foo(13);\n    Bar(13);\n}\n"
)]
#[case::assign_later_single_usage_keeps_rest(
    "assign later, one of several usages",
    "function f() {\n    var x : int;\n    x = 13;\n    Foo($0x);\n    Bar(x);\n}\n",
    "function f() {\n    var x : int;\n    x = 13;\n    Foo(13);\n    Bar(x);\n}\n"
)]
#[case::wraps_compound_assignment(
    "assign later, compound value wrapped",
    "function f() {\n    var $0sum : int;\n    sum = a + b;\n    return sum * 2;\n}\n",
    "function f() {\n    return (a + b) * 2;\n}\n"
)]
#[case::multi_name_inline_first(
    "multi-name list, inline the first name",
    "function f() {\n    var marker, line : string;\n    marker = \"x\";\n    line = \"y\";\n    Foo($0marker);\n    Bar(line);\n}\n",
    "function f() {\n    var line : string;\n    line = \"y\";\n    Foo(\"x\");\n    Bar(line);\n}\n"
)]
#[case::multi_name_inline_last(
    "multi-name list, inline a later name",
    "function f() {\n    var marker, line : string;\n    marker = \"x\";\n    line = \"y\";\n    Foo(marker);\n    Bar($0line);\n}\n",
    "function f() {\n    var marker : string;\n    marker = \"x\";\n    Foo(marker);\n    Bar(\"y\");\n}\n"
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
#[case::read_before_assignment(
    "read precedes the only assignment",
    "function f() {\n    var x : int;\n    Foo($0x);\n    x = 13;\n}\n"
)]
#[case::conditional_assignment(
    "assignment is not an unconditional sibling",
    "function f() {\n    var x : int;\n    if (c) { x = 13; }\n    return $0x;\n}\n"
)]
#[case::two_assignments(
    "more than one assignment",
    "function f() {\n    var x : int;\n    x = 1;\n    x = 2;\n    return $0x;\n}\n"
)]
fn refuses(#[case] label: &str, #[case] src: &str) {
    assert!(
        inlined(src).is_none(),
        "case {label}: expected no inlining offered"
    );
}
