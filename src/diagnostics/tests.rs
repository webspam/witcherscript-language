use rstest::rstest;

use super::collect_diagnostics;
use crate::test_support::TestDb;

#[rstest]
#[case::accepts_local_vars_before_statements(
    "function Ok() {\n var a : int;\n // comment\n a = 1;\n}\n"
)]
#[case::accepts_non_ternary_expression("function Ok() {\n  var x : int;\n  x = 1;\n}\n")]
fn does_not_fire(#[case] source: &str) {
    let t = TestDb::new(source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), source);
    assert!(diagnostics.is_empty(), "got: {diagnostics:#?}");
}

#[test]
fn reports_local_vars_after_statements() {
    let source = "function Bad() {\n a = 1;\n // comment\n var b : int;\n}\n";
    let t = TestDb::new(source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), source);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, "late_local_var_decl");
}

#[test]
fn reports_ternary_expression() {
    let source = "function Pick() {\n  var x : int;\n  x = true ? 1 : 2;\n}\n";
    let t = TestDb::new(source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), source);

    let ternary = diagnostics.iter().find(|d| d.kind == "ternary_cond_expr");
    assert!(
        ternary.is_some(),
        "expected ternary_cond_expr diagnostic, got: {diagnostics:#?}"
    );
    let d = ternary.unwrap();
    assert_eq!(d.start.row, 2);
    assert_eq!(d.start.row, d.end.row);
}

#[test]
fn reports_incomplete_member_access() {
    let source = "class C extends CR4Player {\n  var x : W3AbilityManager;\n  function F() {\n    x = super.\n  }\n}\n";
    let t = TestDb::new(source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), source);

    let incomplete = diagnostics
        .iter()
        .find(|d| d.kind == "incomplete_member_access_expr");
    assert!(
        incomplete.is_some(),
        "expected incomplete_member_access_expr diagnostic"
    );
    let d = incomplete.unwrap();
    assert_eq!(d.start.row, 3);
    assert_eq!(d.start.row, d.end.row);
}
