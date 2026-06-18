use rstest::rstest;

use super::collect_struct_temp_member_diagnostics;
use crate::test_support::TestDb;

#[rstest]
#[case::struct_var_property(
    "struct Vec3 { var Z : float; } function Test() { var v : Vec3; var f : float; f = v.Z; }\n"
)]
#[case::class_field_through_call(
    "class Comp { var name : int; } function Get() : Comp {} function Test() { var n : int; n = Get().name; }\n"
)]
#[case::unknown_property_on_temporary(
    "struct Vec3 { var Z : float; } function Get() : Vec3 {} function Test() { var f : float; f = Get().Nope; }\n"
)]
#[case::method_on_returned_class(
    "class Comp { function Run() {} } function Get() : Comp {} function Test() { Get().Run(); }\n"
)]
#[case::unknown_receiver(
    "function Get() : Mystery {} function Test() { var f : float; f = Get().Z; }\n"
)]
fn does_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_struct_temp_member_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn flags_property_on_struct_returned_by_call() {
    let t = TestDb::new(
        "struct Vec3 { var Z : float; } function GetVec() : Vec3 {} \
         function Test() { var f : float; f = GetVec().Z; }\n",
    );
    let result = collect_struct_temp_member_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "struct_property_on_temporary");
    assert!(diags[0].message.contains("'Z'"));
    assert!(diags[0].message.contains("Vec3"));
}

#[test]
fn flags_property_on_chained_method_call() {
    let t = TestDb::new(
        "//- /a.ws\nstruct Vec3 { var Z : float; }\n\
         //- /b.ws\nclass Comp { function GetPos() : Vec3 {} }\n\
         //- /c.ws\nfunction Test(c : Comp) { var f : float; f = c.GetPos().Z; }\n",
    );
    let result = collect_struct_temp_member_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get("file:///c.ws")
        .expect("c.ws should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "struct_property_on_temporary");
    assert!(diags[0].message.contains("'Z'"));
}
