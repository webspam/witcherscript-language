use rstest::rstest;

use super::{KIND, collect_parent_outside_state_diagnostics};
use crate::test_support::TestDb;

#[rstest]
#[case::parent_in_class_method("class C { function F() { parent.M(); } }\n")]
#[case::virtual_parent_in_class_method("class C { function F() { virtual_parent.M(); } }\n")]
#[case::parent_in_free_function("function F() { parent.M(); }\n")]
#[case::parent_in_struct_method("struct S { function F() { parent.M(); } }\n")]
fn flags_parent_outside_state(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_parent_outside_state_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).expect("expected diagnostic");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, KIND);
}

#[rstest]
#[case::parent_in_state_method(
    "class Owner {} state S in Owner { function F() { parent.M(); } }\n"
)]
#[case::virtual_parent_in_state_method(
    "class Owner {} state S in Owner { function F() { virtual_parent.M(); } }\n"
)]
#[case::wrapmethod_on_state(
    "statemachine class Owner {} state S in Owner { function M() {} }\n\
     @wrapMethod(S) function M() { parent.M(); }\n"
)]
fn does_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_parent_outside_state_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}
