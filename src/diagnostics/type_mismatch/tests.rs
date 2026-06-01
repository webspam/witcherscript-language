use rstest::rstest;

use super::collect_type_mismatch_diagnostics;
use crate::test_support::TestDb;

#[rstest]
#[case::matching_var_init("function Test() { var i : int = 5; }\n")]
#[case::allowed_to_string("function Test() { var s : string; var f : float; s = f; }\n")]
#[case::allowed_widening("function Test() { var f : float; var i : int; f = i; }\n")]
#[case::subtype_upcast(
    "class Base {} class Derived extends Base {} \
     function Test() { var b : Base; var d : Derived; b = d; }\n"
)]
#[case::unresolved_value_suppresses("function Test() { var i : int = Mystery; }\n")]
#[case::binary_op_value_suppresses(
    "function Test() { var a : int; var b : int; var i : int = a + b; }\n"
)]
#[case::null_to_class("class Foo {} function Test() { var f : Foo; f = NULL; }\n")]
fn does_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn flags_incompatible_var_initializer() {
    let t = TestDb::new("function Test() { var i : int = \"x\"; }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
    assert!(diags[0].message.contains("string"));
    assert!(diags[0].message.contains("int"));
}

#[test]
fn flags_incompatible_assignment() {
    let t = TestDb::new("function Test() { var i : int; var f : float; i = f; }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
    assert!(diags[0].message.contains("float"));
    assert!(diags[0].message.contains("int"));
}

#[test]
fn flags_null_into_primitive() {
    let t = TestDb::new("function Test() { var i : int = NULL; }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
}

#[test]
fn does_not_fire_inside_parse_error() {
    let t = TestDb::new("function Test() { do var i : int = \"x\"; }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}
