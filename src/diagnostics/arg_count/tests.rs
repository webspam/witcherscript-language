use rstest::rstest;

use super::collect_arg_count_diagnostics;
use crate::test_support::TestDb;

#[rstest]
#[case::exact("function F(a : int) {} function Test() { F(5); }\n")]
#[case::zero_params_zero_args("function F() {} function Test() { F(); }\n")]
#[case::optional_omitted("function F(a : int, optional b : int) {} function Test() { F(5); }\n")]
#[case::optional_provided(
    "function F(a : int, optional b : int) {} function Test() { F(5, 6); }\n"
)]
#[case::method_exact(
    "class Foo { function M(x : int) {} } function Test() { var f : Foo; f.M(5); }\n"
)]
#[case::unresolved_callee("function Test() { Mystery(1, 2, 3); }\n")]
#[case::all_optional_omitted("function F(optional a : int) {} function Test() { F(); }\n")]
#[case::struct_constructor_not_checked(
    "struct S { var x : int; var y : int; } function Test() { var s : S; s = S(1); }\n"
)]
#[case::hole_skips_optional(
    "function F(a : int, optional b : int, c : int) {} function Test() { F(1,,3); }\n"
)]
fn does_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_arg_count_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

fn single_diagnostic(fixture: &str) -> crate::diagnostics::WorkspaceDiagnostic {
    let t = TestDb::new(fixture);
    let mut result = collect_arg_count_diagnostics(&t.search_docs(), &t.db());
    let mut diags = result
        .remove(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic");
    let diag = diags.pop().unwrap();
    assert_eq!(diag.kind, "arg_count_mismatch");
    diag
}

#[test]
fn flags_too_few_names_the_missing_param() {
    let diag = single_diagnostic("function F(a : int, b : int) {} function Test() { F(5); }\n");
    assert!(diag.message.contains("missing"), "got {:?}", diag.message);
    assert!(diag.message.contains('b'), "got {:?}", diag.message);
}

#[test]
fn flags_too_many_arguments() {
    let diag = single_diagnostic("function F(a : int) {} function Test() { F(5, 6); }\n");
    assert!(diag.message.contains("at most"), "got {:?}", diag.message);
    assert!(diag.message.contains('1'), "got {:?}", diag.message);
    assert!(diag.message.contains('2'), "got {:?}", diag.message);
}

#[test]
fn flags_no_args_when_some_required() {
    let diag = single_diagnostic("function F(a : int) {} function Test() { F(); }\n");
    assert!(diag.message.contains("missing"), "got {:?}", diag.message);
}

#[test]
fn flags_args_to_zero_param_function() {
    let diag = single_diagnostic("function F() {} function Test() { F(1); }\n");
    assert!(diag.message.contains("at most"), "got {:?}", diag.message);
}

#[test]
fn flags_too_few_method_arguments() {
    let diag = single_diagnostic(
        "class Foo { function M(x : int, y : int) {} } function Test() { var f : Foo; f.M(5); }\n",
    );
    assert!(diag.message.contains("missing"), "got {:?}", diag.message);
    assert!(diag.message.contains('y'), "got {:?}", diag.message);
}

#[test]
fn flags_below_required_with_optional_present() {
    let diag =
        single_diagnostic("function F(a : int, optional b : int) {} function Test() { F(); }\n");
    assert!(diag.message.contains("missing"), "got {:?}", diag.message);
    assert!(diag.message.contains('a'), "got {:?}", diag.message);
}

#[test]
fn flags_above_max_with_optional_present() {
    let diag = single_diagnostic(
        "function F(a : int, optional b : int) {} function Test() { F(1, 2, 3); }\n",
    );
    assert!(diag.message.contains("at most"), "got {:?}", diag.message);
}

#[test]
fn flags_hole_at_required_position() {
    let diag = single_diagnostic(
        "function F(a : int, b : int, optional c : int) {} function Test() { F(1,,3); }\n",
    );
    assert!(diag.message.contains("missing"), "got {:?}", diag.message);
    assert!(diag.message.contains('b'), "got {:?}", diag.message);
}

#[test]
fn flags_leading_hole_at_required_position() {
    let diag =
        single_diagnostic("function F(a : int, optional b : int) {} function Test() { F(,2); }\n");
    assert!(diag.message.contains("missing"), "got {:?}", diag.message);
    assert!(diag.message.contains('a'), "got {:?}", diag.message);
}

#[test]
fn does_not_fire_inside_parse_error() {
    let t = TestDb::new("function F(a : int) {} function Test() { do F(); }\n");
    let result = collect_arg_count_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn surfaces_through_aggregate_pipeline() {
    let t = TestDb::new("function F(a : int) {} function Test() { F(); }\n");
    let diags = crate::diagnostics::collect_cst_diagnostics_for_document(
        t.primary_uri(),
        t.primary_doc(),
        &t.db(),
    );
    assert!(
        diags.iter().any(|d| d.kind == "arg_count_mismatch"),
        "expected an arg_count_mismatch from the registered rule set, got {diags:?}"
    );
}
