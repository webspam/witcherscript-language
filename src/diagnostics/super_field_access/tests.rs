use rstest::rstest;

use super::collect_super_field_access_diagnostics;
use crate::test_support::TestDb;

#[rstest]
#[case::field_read(
    "class Base { var x : int; } \
     class Derived extends Base { function F() { var y : int; y = super.x; } }\n"
)]
#[case::field_assignment_target(
    "class Base { var x : int; } \
     class Derived extends Base { function F() { super.x = 1; } }\n"
)]
fn flags_super_field_access(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_super_field_access_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).expect("expected diagnostic");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "super_field_access");
}

#[rstest]
#[case::super_method_call(
    "class Base { function M() {} } \
     class Derived extends Base { function F() { super.M(); } }\n"
)]
#[case::this_field_access(
    "class Base { var x : int; } \
     class Derived extends Base { function F() { var y : int; y = this.x; } }\n"
)]
#[case::error_subtree(
    "class Base { var x : int; } \
     class Derived extends Base { function F() { y = super. } }\n"
)]
fn does_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_super_field_access_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}
