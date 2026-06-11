use rstest::rstest;

use super::collect_diagnostics;
use crate::test_support::TestDb;

#[rstest]
#[case::accepts_local_vars_before_statements(
    "function Ok() {\n var a : int;\n // comment\n a = 1;\n}\n"
)]
#[case::accepts_non_ternary_expression("function Ok() {\n  var x : int;\n  x = 1;\n}\n")]
#[case::accepts_struct_property_without_access_modifier("struct S {\n  var x : int;\n}\n")]
#[case::accepts_access_modifier_on_class_field("class C {\n  private var x : int;\n}\n")]
#[case::accepts_single_line_string("function F() {\n  var s : string;\n  s = \"a b\";\n}\n")]
#[case::accepts_string_with_escaped_quote(
    "function F() {\n  var s : string;\n  s = \"a \\\" b\";\n}\n"
)]
fn does_not_fire(#[case] source: &str) {
    let t = TestDb::new(source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), source);
    assert!(diagnostics.is_empty(), "got: {diagnostics:#?}");
}

#[rstest]
#[case::private("struct S {\n  private var x : int;\n}\n", "private")]
#[case::protected("struct S {\n  protected var x : int;\n}\n", "protected")]
#[case::public("struct S {\n  public var x : int;\n}\n", "public")]
fn reports_access_modifier_on_struct_property(#[case] source: &str, #[case] keyword: &str) {
    let t = TestDb::new(source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), source);

    let found = diagnostics
        .iter()
        .find(|d| d.kind == "struct_property_access_modifier");
    assert!(
        found.is_some(),
        "expected struct_property_access_modifier for {keyword}, got: {diagnostics:#?}"
    );
    let d = found.unwrap();
    assert_eq!(
        &source[d.byte_range.clone()],
        keyword,
        "diagnostic should underline only the {keyword} keyword"
    );
}

#[rstest]
#[case::max_int("2147483647")]
#[case::min_int("-2147483648")]
#[case::plus_sign("+2147483647")]
#[case::max_hex("0x7FFFFFFF")]
fn accepts_int_literals_in_range(#[case] literal: &str) {
    let source = format!("function F() {{\n  var x : int;\n  x = {literal};\n}}\n");
    let t = TestDb::new(&source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), &source);

    assert!(
        !diagnostics.iter().any(|d| d.kind == "int_overflow"),
        "{literal} should be in range, got: {diagnostics:#?}"
    );
}

#[rstest]
#[case::max_int_plus_one("2147483648", "2147483648")]
#[case::min_int_minus_one("-2147483649", "-2147483649")]
#[case::hex_over_max("0x80000000", "0x80000000")]
#[case::observed_overflow("0xFFFFFFFF8000000", "0xFFFFFFFF8000000")]
#[case::spaced_minus_is_an_operator("- 2147483648", "2147483648")]
fn reports_int_literal_overflow(#[case] literal: &str, #[case] underlined: &str) {
    let source = format!("function F() {{\n  var x : int;\n  x = {literal};\n}}\n");
    let t = TestDb::new(&source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), &source);

    let found = diagnostics.iter().find(|d| d.kind == "int_overflow");
    assert!(
        found.is_some(),
        "expected int_overflow for {literal}, got: {diagnostics:#?}"
    );
    let d = found.unwrap();
    assert_eq!(
        &source[d.byte_range.clone()],
        underlined,
        "diagnostic should underline the literal in {literal}"
    );
}

#[test]
fn reports_string_containing_linefeed() {
    let source = "function F() {\n  var s : string;\n  s = \"a\nb\";\n}\n";
    let t = TestDb::new(source);
    let diagnostics = collect_diagnostics(t.primary_doc().tree.root_node(), source);

    let found = diagnostics.iter().find(|d| d.kind == "string_linefeed");
    assert!(
        found.is_some(),
        "expected string_linefeed diagnostic, got: {diagnostics:#?}"
    );
    let d = found.unwrap();
    assert_eq!(&source[d.byte_range.clone()], "\"a\nb\"");
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
