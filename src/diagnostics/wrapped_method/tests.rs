use rstest::rstest;

use super::collect_wrapped_method_diagnostics;
use crate::test_support::TestDb;

fn kinds(diags: &[super::WorkspaceDiagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.kind.as_str()).collect()
}

#[rstest]
#[case::single_call(
    "class Foo {} \
     @wrapMethod(Foo) function W() { wrappedMethod(); }\n"
)]
#[case::unannotated_function("function F() { wrappedMethod(); }\n")]
#[case::add_method_annotation(
    "class Foo {} \
     @addMethod(Foo) function A() {}\n"
)]
#[case::call_inside_if(
    "class Foo {} \
     @wrapMethod(Foo) function W() { if (true) { wrappedMethod(); } }\n"
)]
fn does_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_wrapped_method_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn missing_call_flagged() {
    let t = TestDb::new(
        "class Foo {} \
         @wrapMethod(Foo) function W() {}\n",
    );
    let result = collect_wrapped_method_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).unwrap();
    assert_eq!(kinds(diags), vec!["missing_wrapped_method"]);
    assert!(diags[0].message.contains('W'));
}

#[test]
fn duplicate_calls_flagged() {
    let t = TestDb::new(
        "class Foo {} \
         @wrapMethod(Foo) function W() { wrappedMethod(); wrappedMethod(); wrappedMethod(); }\n",
    );
    let result = collect_wrapped_method_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).unwrap();
    assert_eq!(
        kinds(diags),
        vec!["duplicate_wrapped_method", "duplicate_wrapped_method"]
    );
}

#[test]
fn member_access_does_not_count() {
    let t = TestDb::new(
        "class Foo {} \
         @wrapMethod(Foo) function W() { this.wrappedMethod(); }\n",
    );
    let result = collect_wrapped_method_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).unwrap();
    assert_eq!(kinds(diags), vec!["missing_wrapped_method"]);
}
