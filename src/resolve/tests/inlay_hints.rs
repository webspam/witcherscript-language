use rstest::rstest;

use crate::line_index::{SourcePosition, SourceRange};
use crate::resolve::inlay_hints;
use crate::test_support::TestDb;

fn full_range() -> SourceRange {
    SourceRange {
        start: SourcePosition {
            line: 0,
            character: 0,
        },
        end: SourcePosition {
            line: u32::MAX,
            character: 0,
        },
    }
}

fn hint_labels(source: &str) -> Vec<String> {
    let t = TestDb::new(source);
    inlay_hints(
        t.primary_uri(),
        t.primary_doc(),
        &t.db(),
        full_range(),
        &|| true,
    )
    .expect("an uncancellable walk yields Some")
    .into_iter()
    .map(|h| h.label)
    .collect()
}

#[rstest]
#[case::two_params(
    "function Find(name : string, count : int) {}\nfunction Caller() { Find(\"a\", 1); }\n",
    &["name:", "count:"],
)]
#[case::suppress_matching_identifier(
    "function Foo(target : int) {}\nfunction Caller() { Foo(target); }\n",
    &[],
)]
#[case::no_suppress_when_arg_differs(
    "function Foo(target : int) {}\nfunction Caller() { Foo(other); }\n",
    &["target:"],
)]
#[case::method_call(
    "class C { function F(modifier : float) {} }\nfunction Caller() { var c : C; c.F(1.0); }\n",
    &["modifier:"],
)]
#[case::out_param_marked(
    "function G(out result : int) {}\nfunction Caller() { G(x); }\n",
    &["out result:"],
)]
#[case::out_param_not_suppressed_on_name_match(
    "function G(out target : int) {}\nfunction Caller() { G(target); }\n",
    &["out target:"],
)]
#[case::optional_param_labelled(
    "function H(name : string, optional count : int) {}\nfunction Caller() { H(\"a\", 1); }\n",
    &["name:", "count:"],
)]
#[case::arity_mismatch_zips_to_shorter(
    "function J(a : int, b : int) {}\nfunction Caller() { J(1); }\n",
    &["a:"],
)]
#[case::empty_slot_skips_call(
    "function K(a : int, b : int) {}\nfunction Caller() { K(1,,2); }\n",
    &[],
)]
#[case::unresolved_callee_skipped("function Caller() { NoSuchFn(1); }\n", &[])]
#[case::nested_calls(
    "function Inner(inner : int) : int {}\nfunction Outer(outer : int) {}\nfunction Caller() { Outer(Inner(1)); }\n",
    &["outer:", "inner:"],
)]
fn parameter_hint_labels(#[case] source: &str, #[case] expected: &[&str]) {
    let expected: Vec<String> = expected.iter().map(std::string::ToString::to_string).collect();
    assert_eq!(
        hint_labels(source),
        expected,
        "labels for source: {source:?}"
    );
}

#[test]
fn range_excludes_calls_outside_viewport() {
    let t = TestDb::new(
        "function Near(near : int) {}\n\
         function Far(far : int) {}\n\
         function Caller() {\n\
         Near(1);\n\
         Far(2);\n\
         }\n",
    );
    let doc = t.primary_doc();
    let cut_byte = doc
        .source
        .find("Far(2)")
        .expect("fixture contains the Far call");
    let view = SourceRange {
        start: SourcePosition {
            line: 0,
            character: 0,
        },
        end: doc.line_index.byte_to_position(&doc.source, cut_byte),
    };
    let labels: Vec<String> = inlay_hints(t.primary_uri(), doc, &t.db(), view, &|| true)
        .expect("an uncancellable walk yields Some")
        .into_iter()
        .map(|h| h.label)
        .collect();
    assert_eq!(
        labels,
        vec!["near:".to_string()],
        "only the in-viewport call should be hinted"
    );
}
