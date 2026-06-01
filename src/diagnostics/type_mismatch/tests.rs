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
#[case::matching_arg("function TakesInt(x : int) {} function Test() { TakesInt(5); }\n")]
#[case::implicit_cast_arg(
    "function TakesString(s : string) {} function Test() { TakesString(1.0); }\n"
)]
#[case::omitted_optional_arg(
    "function F(a : int, optional b : int) {} function Test() { F(5); }\n"
)]
#[case::subtype_arg(
    "class Base {} class Derived extends Base {} \
     function Take(b : Base) {} function Test() { var d : Derived; Take(d); }\n"
)]
#[case::method_arg_ok(
    "class Foo { function M(x : int) {} } function Test() { var f : Foo; f.M(5); }\n"
)]
#[case::unresolved_callee("function Test() { Mystery(\"s\"); }\n")]
#[case::array_method_no_false_positive(
    "function Test() { var a : array<int>; a.PushBack(\"s\"); }\n"
)]
#[case::matching_return("function F() : int { return 5; }\n")]
#[case::implicit_cast_return("function F() : string { return 1.0; }\n")]
#[case::void_return("function F() { return; }\n")]
#[case::subtype_return(
    "class Base {} class Derived extends Base {} \
     function F() : Base { var d : Derived; return d; }\n"
)]
#[case::matching_default("class C { var n : int; default n = 5; }\n")]
#[case::matching_defaults_block("class C { var n : int; defaults { n = 5; } }\n")]
#[case::name_default_name_literal("class C { var n : name; default n = 'Swimming'; }\n")]
#[case::compound_widening("function Test() { var f : float; f += 1; }\n")]
#[case::compound_to_string("function Test() { var s : string; s += 5; }\n")]
#[case::compound_object_skipped(
    "class Foo {} function Test() { var a : Foo; var b : Foo; a += b; }\n"
)]
#[case::object_to_bool("class Foo {} function Test() { var f : Foo; var b : bool; b = f; }\n")]
#[case::object_to_string("class Foo {} function Test() { var f : Foo; var s : string; s = f; }\n")]
#[case::enum_to_string(
    "enum Mood { Happy } function Test() { var m : Mood; var s : string; s = m; }\n"
)]
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
fn flags_string_literal_into_name_var_initializer() {
    let t = TestDb::new("function Test() { var n : name = \"Swimming\"; }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
    assert!(diags[0].message.contains("string"));
    assert!(diags[0].message.contains("name"));
}

#[rstest]
#[case::default("class C { var n : name; default n = \"Swimming\"; }\n")]
#[case::defaults_block("class C { var n : name; defaults { n = \"Swimming\"; } }\n")]
fn string_name_default_is_info_not_error(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have a diagnostic");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "string_as_name_default");
    assert_eq!(diags[0].severity, super::Severity::Info);
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
fn flags_incompatible_argument() {
    let t = TestDb::new("function TakesInt(x : int) {} function Test() { TakesInt(\"s\"); }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
    assert!(diags[0].message.contains("Argument 1"));
    assert!(diags[0].message.contains("int"));
    assert!(diags[0].message.contains("string"));
}

#[test]
fn flags_incompatible_method_argument() {
    let t = TestDb::new(
        "class Foo { function M(x : int) {} } function Test() { var f : Foo; f.M(\"s\"); }\n",
    );
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
    assert!(diags[0].message.contains("Argument 1"));
}

#[test]
fn flags_second_argument_only() {
    let t = TestDb::new("function F(a : int, b : int) {} function Test() { F(5, \"s\"); }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("Argument 2"));
}

#[test]
fn flags_incompatible_return() {
    let t = TestDb::new("function F() : int { return \"s\"; }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
    assert!(diags[0].message.contains("return"));
    assert!(diags[0].message.contains("string"));
    assert!(diags[0].message.contains("int"));
}

#[test]
fn flags_incompatible_default() {
    let t = TestDb::new("class C { var n : int; default n = \"x\"; }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
}

#[test]
fn flags_incompatible_defaults_block() {
    let t = TestDb::new("class C { var n : int; defaults { n = \"x\"; } }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "type_mismatch");
}

#[test]
fn flags_incompatible_compound_assignment() {
    let t = TestDb::new("function Test() { var i : int; i += \"s\"; }\n");
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
fn does_not_fire_inside_parse_error() {
    let t = TestDb::new("function Test() { do var i : int = \"x\"; }\n");
    let result = collect_type_mismatch_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn surfaces_through_aggregate_pipeline() {
    let t = TestDb::new("function Test() { var i : int = \"x\"; }\n");
    let diags = crate::diagnostics::collect_cst_diagnostics_for_document(
        t.primary_uri(),
        t.primary_doc(),
        &t.db(),
    );
    assert!(
        diags.iter().any(|d| d.kind == "type_mismatch"),
        "expected a type_mismatch from the registered rule set, got {diags:?}"
    );
}
