use rstest::rstest;

use super::{KIND, collect_unused_symbol_diagnostics};
use crate::diagnostics::{Severity, WorkspaceDiagnostic};
use crate::test_support::TestDb;

fn primary_diags(t: &TestDb) -> Vec<WorkspaceDiagnostic> {
    collect_unused_symbol_diagnostics(&t.search_docs(), &t.db())
        .get(t.primary_uri())
        .cloned()
        .unwrap_or_default()
}

#[rstest]
#[case::single_param(
    "function F(foo : int) {}\n//         ^^^^^^^^^ u\n",
    "Parameter 'foo' is never used"
)]
#[case::out_param_skips_specifiers(
    "function F(out foo : int) {}\n//             ^^^^^^^^^ u\n",
    "Parameter 'foo' is never used"
)]
#[case::grouped_param_dims_ident_only(
    "function G(a, bbb, c : int) { c = a; }\n//            ^^^ u\n",
    "Parameter 'bbb' is never used"
)]
#[case::local_single_no_init(
    "function H() {\n  var x : int;\n//^^^^^^^^^^^^ u\n}\n",
    "Local variable 'x' is never used"
)]
#[case::local_single_constant_init_fades_value(
    "function I() {\n  var x : int = 5;\n//^^^^^^^^^^^^^^^^ u\n}\n",
    "Local variable 'x' is never used"
)]
#[case::local_single_computed_init_stays_bright(
    "function M() {\n  var s : string = \"a\" + \"b\";\n//^^^^^^^^^^^^^^^ u\n}\n",
    "Local variable 's' is never used"
)]
#[case::local_single_reference_init_stays_bright(
    "function L(p : int) {\n  var x : int = p;\n//^^^^^^^^^^^^ u\n}\n",
    "Local variable 'x' is never used"
)]
#[case::local_list_all_no_init(
    "function J() {\n  var a, b : int;\n//^^^^^^^^^^^^^^^ u\n}\n",
    "Local variables 'a', 'b' are never used"
)]
#[case::local_list_all_constant_init(
    "function K() {\n  var a, b : int = 5;\n//^^^^^^^^^^^^^^^^^^^ u\n}\n",
    "Local variables 'a', 'b' are never used"
)]
#[case::local_list_partial_dims_name_and_comma(
    "function P() {\n  var a, bbb, c : int;\n//       ^^^^ u\n  a = c;\n}\n",
    "Local variable 'bbb' is never used"
)]
#[case::private_field_whole_statement(
    "class C {\n  private var t : int;\n//^^^^^^^^^^^^^^^^^^^^ u\n}\n",
    "Field 't' is never used"
)]
#[case::field_default_is_not_a_reference(
    "class C {\n  private var t : int;\n//^^^^^^^^^^^^^^^^^^^^ u\n  default t = 1;\n}\n",
    "Field 't' is never used"
)]
fn dims_unused_binding(#[case] fixture: &str, #[case] expected_message: &str) {
    let t = TestDb::new(fixture);
    let diags = primary_diags(&t);

    assert_eq!(
        diags.len(),
        1,
        "case message {expected_message:?}: one diagnostic"
    );
    assert_eq!(diags[0].kind, KIND, "case {expected_message:?}: kind");
    assert_eq!(
        diags[0].message, expected_message,
        "case {expected_message:?}: message"
    );
    assert_eq!(
        diags[0].range,
        t.span("u").1,
        "case {expected_message:?}: dimmed range"
    );
}

#[rstest]
#[case::used_param("function F(foo : int) { foo = 1; }\n")]
#[case::used_local("function H() { var x : int; x = 1; }\n")]
#[case::public_field("class C { var open : int; }\n")]
#[case::protected_field("class C { protected var p : int; }\n")]
#[case::struct_field("struct S { var x : int; }\n")]
#[case::add_field_injection("class C {}\n@addField(C) var injected : int;\n")]
#[case::field_used_in_method("class C { private var f : int; function g() { f = 1; } }\n")]
fn keeps_used_or_out_of_scope_bindings(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    assert!(
        collect_unused_symbol_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "fixture {fixture:?} must not dim anything",
    );
}

#[test]
fn unused_diagnostics_use_hint_severity() {
    let t = TestDb::new("function F(foo : int) {}\n");
    let diags = primary_diags(&t);
    assert_eq!(diags.len(), 1, "one diagnostic");
    assert_eq!(
        diags[0].severity,
        Severity::Hint,
        "unused dimming must be a hint so it fades without a squiggle",
    );
}
