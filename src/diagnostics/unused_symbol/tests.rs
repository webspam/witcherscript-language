use std::cmp::Reverse;
use std::ops::Range;

use rstest::rstest;

use super::{KIND, collect_unused_symbol_diagnostics};
use crate::diagnostics::{Severity, WorkspaceDiagnostic};
use crate::line_index::SourcePosition;
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
#[case::single_param_fades_specifiers(
    "function F(optional out foo : int) {}\n//         ^^^^^^^^^^^^^^^^^^^^^^ u\n",
    "Parameter 'foo' is never used"
)]
#[case::grouped_param_dims_ident_only(
    "function G(a, bbb, c : int) { c = a; }\n//            ^^^ u\n",
    "Parameter 'bbb' is never used"
)]
#[case::all_grouped_params_fade_whole_group(
    "function G(a, b : int) {}\n//         ^^^^^^^^^^ u\n",
    "Parameters 'a', 'b' are never used"
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
#[case::import_function_params("import function F(x : int, y : string) : void;\n")]
#[case::bodyless_method("class C { import function M(a : int) : void; }\n")]
fn keeps_used_or_out_of_scope_bindings(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    assert!(
        collect_unused_symbol_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "fixture {fixture:?} must not dim anything",
    );
}

fn apply_removal(t: &TestDb) -> String {
    let diags = primary_diags(t);
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic to remove");
    let doc = t.primary_doc();
    let data = diags[0]
        .data
        .as_ref()
        .expect("an unused diagnostic carries removal data");
    let ranges = data["removeRanges"]
        .as_array()
        .expect("removeRanges is an array");
    let field = |value: &serde_json::Value| usize::try_from(value.as_u64().unwrap()).unwrap();
    let to_byte = |value: &serde_json::Value| {
        let pos = SourcePosition::new(field(&value["line"]), field(&value["character"]));
        doc.line_index
            .position_to_byte(&doc.source, pos)
            .expect("removal position is in bounds")
    };
    let mut byte_ranges: Vec<Range<usize>> = ranges
        .iter()
        .map(|r| to_byte(&r["start"])..to_byte(&r["end"]))
        .collect();
    // Splice rightmost-first so earlier deletions do not shift later byte offsets.
    byte_ranges.sort_by_key(|r| Reverse(r.start));
    let mut source = doc.source.clone();
    for range in byte_ranges {
        source.replace_range(range, "");
    }
    source
}

#[rstest]
#[case::single_param("function F(foo : int) {}\n", "function F() {}\n")]
#[case::lone_param_with_specifiers("function F(optional out foo : int) {}\n", "function F() {}\n")]
#[case::partial_param_keeps_others(
    "function G(a, bbb, c : int) { c = a; }\n",
    "function G(a, c : int) { c = a; }\n"
)]
#[case::partial_param_last_name(
    "function G(a, b, ccc : int) { a = b; }\n",
    "function G(a, b : int) { a = b; }\n"
)]
#[case::whole_param_group(
    "function G(a : int, b : float) { b = 1; }\n",
    "function G(b : float) { b = 1; }\n"
)]
#[case::last_param_group(
    "function G(a : int, b : float) { a = 1; }\n",
    "function G(a : int) { a = 1; }\n"
)]
#[case::all_grouped_params("function G(a, b : int) {}\n", "function G() {}\n")]
#[case::local_no_init("function H() {\n  var x : int;\n}\n", "function H() {\n}\n")]
#[case::local_with_init("function I() {\n  var x : int = 5;\n}\n", "function I() {\n}\n")]
#[case::partial_local_keeps_others(
    "function P() {\n  var a, bbb, c : int;\n  a = c;\n}\n",
    "function P() {\n  var a, c : int;\n  a = c;\n}\n"
)]
#[case::private_field("class C {\n  private var t : int;\n}\n", "class C {\n}\n")]
#[case::field_with_default(
    "class C {\n  private var t : int;\n  default t = 1;\n}\n",
    "class C {\n}\n"
)]
#[case::field_with_default_block(
    "class C {\n  private var t : int;\n  defaults {\n    t = 1;\n  }\n}\n",
    "class C {\n  defaults {\n  }\n}\n"
)]
#[case::field_default_for_other_name_kept(
    "class C {\n  private var a, bbb : int;\n  default a = 1;\n  default bbb = 2;\n  function g() { a = 0; }\n}\n",
    "class C {\n  private var a : int;\n  default a = 1;\n  function g() { a = 0; }\n}\n"
)]
fn removal_cleans_source(#[case] src: &str, #[case] expected: &str) {
    let t = TestDb::new(src);
    assert_eq!(apply_removal(&t), expected, "removal of {src:?}");
}

#[test]
fn removal_data_carries_noun() {
    struct Case {
        src: &'static str,
        noun: &'static str,
    }
    let cases = [
        Case {
            src: "function F(foo : int) {}\n",
            noun: "param",
        },
        Case {
            src: "function G(a, b : int) {}\n",
            noun: "params",
        },
        Case {
            src: "function H() {\n  var x : int;\n}\n",
            noun: "var",
        },
        Case {
            src: "class C {\n  private var t : int;\n}\n",
            noun: "field",
        },
    ];
    for case in cases {
        let t = TestDb::new(case.src);
        let diags = primary_diags(&t);
        assert_eq!(diags.len(), 1, "case {:?}: one diagnostic", case.src);
        assert_eq!(
            diags[0].data.as_ref().unwrap()["noun"],
            case.noun,
            "case {:?}: noun",
            case.src
        );
    }
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
