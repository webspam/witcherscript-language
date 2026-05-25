use rstest::rstest;

use super::collect_unknown_method_diagnostics;
use crate::test_support::TestDb;

#[rstest]
#[case::known_method("class Foo { function Bar() {} function Test() { var f : Foo; f.Bar(); } }\n")]
#[case::inherited_method(
    "//- /a.ws\nclass Base { function Inherited() {} }\n\
     //- /b.ws\nclass Child extends Base { function Test() { var c : Child; c.Inherited(); } }\n"
)]
#[case::this_known_method("class Foo { function Bar() {} function Run() { this.Bar(); } }\n")]
#[case::unknown_receiver_type("function Test(x : Unknown) { x.Method(); }\n")]
#[case::primitive_receiver("function Test() { var n : int; n.Method(); }\n")]
#[case::private_method_inside_class(
    "class Foo { private function Secret() {} function Test() { var f : Foo; f.Secret(); } }\n"
)]
fn does_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_unknown_method_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn flags_unknown_method_on_known_type() {
    let t = TestDb::new("class Foo { } function Test() { var f : Foo; f.Qux(); }\n");
    let result = collect_unknown_method_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
    assert!(diags[0].message.contains("Qux"));
    assert!(diags[0].message.contains("Foo"));
}

#[test]
fn flags_this_unknown_method() {
    let t = TestDb::new("class Foo { function Run() { this.Nonexistent(); } }\n");
    let result = collect_unknown_method_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
}

#[test]
fn flags_struct_receiver() {
    let t = TestDb::new("struct Vec3 { } function Test() { var v : Vec3; v.Normalize(); }\n");
    let result = collect_unknown_method_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get(t.primary_uri())
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
}

#[test]
fn flags_private_method_call_from_outside_class() {
    let t = TestDb::new(
        "class Foo { private function Secret() {} } \
         function Run() { var f : Foo; f.Secret(); }\n",
    );
    let result = collect_unknown_method_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get(t.primary_uri()).unwrap();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "private_member_access");
    assert!(diags[0].message.contains("Secret"));
    assert!(diags[0].message.contains("'Foo'"));
}

#[test]
fn flags_unknown_method_cross_file() {
    let t = TestDb::new(concat!(
        "//- /types.ws\n",
        "class Widget { function Draw() {} }\n",
        "//- /use.ws\n",
        "function Test() { var w : Widget; w.Render(); }\n",
    ));
    let result = collect_unknown_method_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get("file:///use.ws")
        .expect("use.ws should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
    assert!(diags[0].message.contains("Render"));
}

#[test]
fn flags_chained_call_unknown() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Builder { function Build() : Result {} }\n",
        "//- /b.ws\n",
        "class Result { }\n",
        "//- /c.ws\n",
        "function Test() { var b : Builder; b.Build().Missing(); }\n",
    ));
    let result = collect_unknown_method_diagnostics(&t.search_docs(), &t.db());

    let diags = result
        .get("file:///c.ws")
        .expect("c.ws should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
    assert!(diags[0].message.contains("Missing"));
}
