use rstest::rstest;

use super::collect_abstract_instantiation_diagnostics;
use crate::test_support::TestDb;

#[rstest]
#[case::single_file(
    "abstract class Base {} \
     function F() { var b : Base; b = new Base in this; }\n",
    "file:///main.ws"
)]
#[case::across_files(
    "//- /a.ws\n\
     abstract class Base {}\n\
     //- /b.ws\n\
     function F() { var b : Base; b = new Base in this; }\n",
    "file:///b.ws"
)]
fn flags_new_on_abstract_class(#[case] fixture: &str, #[case] flagged_uri: &str) {
    let t = TestDb::new(fixture);
    let result = collect_abstract_instantiation_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(flagged_uri).expect("expected diagnostic");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "abstract_instantiation");
    assert!(diags[0].message.contains("Base"));
}

#[rstest]
#[case::concrete_class(
    "class Concrete {} \
     function F() { var c : Concrete; c = new Concrete in this; }\n"
)]
#[case::unknown_class("function F() { var x : Missing; x = new Missing in this; }\n")]
fn does_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_abstract_instantiation_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}
