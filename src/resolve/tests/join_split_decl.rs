use rstest::rstest;

use crate::resolve::extract_common::apply_splices;
use crate::resolve::{join_declaration, split_declaration};
use crate::test_support::TestDb;

fn joined(src: &str) -> Option<String> {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let byte = doc.line_index.position_to_byte(&doc.source, pos)?;
    let edits = join_declaration(&uri, doc, &t.db(), byte)?;
    Some(apply_splices(&doc.source, &edits))
}

fn split(src: &str) -> Option<String> {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let byte = doc.line_index.position_to_byte(&doc.source, pos)?;
    let edits = split_declaration(doc, byte)?;
    Some(apply_splices(&doc.source, &edits))
}

#[rstest]
#[case::from_declaration(
    "cursor on the declaration",
    "function f() {\n    var $0one : float;\n    var two : int;\n    one = 1;\n    two = 2;\n}\n",
    "function f() {\n    var one : float = 1;\n    var two : int;\n    two = 2;\n}\n"
)]
#[case::from_assignment(
    "cursor on the assignment",
    "function f() {\n    var one : float;\n    var two : int;\n    $0one = 1;\n    two = 2;\n}\n",
    "function f() {\n    var one : float = 1;\n    var two : int;\n    two = 2;\n}\n"
)]
#[case::adjacent(
    "assignment immediately follows the declaration",
    "function f() {\n    var $0x : int;\n    x = 5;\n}\n",
    "function f() {\n    var x : int = 5;\n}\n"
)]
#[case::effectful_value_adjacent(
    "a side-effecting value is fine with nothing to cross",
    "function f() {\n    var $0x : int;\n    x = Compute();\n}\n",
    "function f() {\n    var x : int = Compute();\n}\n"
)]
#[case::operand_in_scope_pure_intervening(
    "an operand stays valid across a pure declaration",
    "function f(a : int) {\n    var $0x : int;\n    var y : int;\n    x = a;\n    y = 2;\n}\n",
    "function f(a : int) {\n    var x : int = a;\n    var y : int;\n    y = 2;\n}\n"
)]
fn joins(#[case] label: &str, #[case] src: &str, #[case] expected: &str) {
    let got = joined(src).unwrap_or_else(|| panic!("case {label}: expected a join"));
    assert_eq!(got, expected, "case {label}: joined output mismatch");
}

#[rstest]
#[case::already_initialised(
    "declaration already has an initializer",
    "function f() {\n    var $0x : int = 1;\n    x = 2;\n}\n"
)]
#[case::multi_name_from_decl(
    "multi-name declaration, cursor on declaration",
    "function f() {\n    var $0a, b : int;\n    a = 1;\n}\n"
)]
#[case::multi_name_from_assignment(
    "multi-name declaration, cursor on assignment",
    "function f() {\n    var a, b : int;\n    $0a = 1;\n}\n"
)]
#[case::compound_assignment(
    "first write is a compound assignment",
    "function f() {\n    var $0x : int;\n    x += 1;\n}\n"
)]
#[case::member_target(
    "first write targets a member of the local",
    "function f() {\n    var $0p : Foo;\n    p.field = 1;\n}\n"
)]
#[case::read_between(
    "the local is read before the assignment",
    "function f() {\n    var $0x : int;\n    Foo(x);\n    x = 1;\n}\n"
)]
#[case::value_references_target(
    "the value reads the local being initialised",
    "function f() {\n    var $0x : int;\n    x = x + 1;\n}\n"
)]
#[case::operand_reassigned_in_window(
    "an operand is reassigned before the assignment",
    "function f() {\n    var a : int = 1;\n    var $0x : int;\n    a = 99;\n    x = a;\n}\n"
)]
#[case::operand_introduced_in_window(
    "an operand is declared between the declaration and the assignment",
    "function f() {\n    var $0x : int;\n    var y : int = 5;\n    x = y;\n}\n"
)]
#[case::effectful_value_crosses_statement(
    "a side-effecting value would reorder past a statement",
    "function f() {\n    var $0x : int;\n    Foo();\n    x = Bar();\n}\n"
)]
#[case::call_in_window_with_operand(
    "a call in the window may mutate the value's operand",
    "function f(a : int) {\n    var $0x : int;\n    Foo();\n    x = a;\n}\n"
)]
#[case::assignment_nested_in_if(
    "the assignment is conditional",
    "function f() {\n    var $0x : int;\n    if (c) { x = 1; }\n    return x;\n}\n"
)]
#[case::no_assignment(
    "the local is never assigned",
    "function f() {\n    var $0x : int;\n    Foo(x);\n}\n"
)]
#[case::later_assignment(
    "cursor on a later assignment, not the first",
    "function f() {\n    var x : int;\n    x = 1;\n    $0x = 2;\n}\n"
)]
fn join_refuses(#[case] label: &str, #[case] src: &str) {
    assert!(
        joined(src).is_none(),
        "case {label}: expected no join offered"
    );
}

#[rstest]
#[case::literal(
    "literal initializer",
    "function f() {\n    var $0x : int = 5;\n}\n",
    "function f() {\n    var x : int;\n    x = 5;\n}\n"
)]
#[case::compound_value(
    "compound initializer",
    "function f(a : int, b : int) {\n    var $0sum : int = a + b;\n}\n",
    "function f(a : int, b : int) {\n    var sum : int;\n    sum = a + b;\n}\n"
)]
fn splits(#[case] label: &str, #[case] src: &str, #[case] expected: &str) {
    let got = split(src).unwrap_or_else(|| panic!("case {label}: expected a split"));
    assert_eq!(got, expected, "case {label}: split output mismatch");
}

#[rstest]
#[case::no_initializer(
    "nothing to split off",
    "function f() {\n    var $0x : int;\n    x = 5;\n}\n"
)]
#[case::multi_name(
    "multi-name initialised declaration",
    "function f() {\n    var $0a, b : int = 0;\n}\n"
)]
fn split_refuses(#[case] label: &str, #[case] src: &str) {
    assert!(
        split(src).is_none(),
        "case {label}: expected no split offered"
    );
}

#[test]
fn split_then_join_round_trips() {
    let original = "function f() {\n    var x : int = 5;\n}\n";
    let split_out = split("function f() {\n    var $0x : int = 5;\n}\n").expect("split");
    let join_in = split_out.replacen("var x", "var $0x", 1);
    let join_out = joined(&join_in).expect("join");
    assert_eq!(
        join_out, original,
        "split then join should reproduce the original"
    );
}
