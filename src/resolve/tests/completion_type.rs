use rstest::rstest;

use super::super::{
    class_header_keyword_completions, completion_members, extends_completions,
    state_owner_completions, type_completions,
};
use crate::line_index::SourcePosition;
use crate::test_support::{def_names, TestDb};

#[rstest]
#[case::right_after_colon_before_any_type_typed(
    "class CTest {}\nclass C {var test:$0}",
    &[], false,
)]
#[case::partial_type_name_in_annotation(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\nfunction Test() {\n  var x : CP$0\n  var y : int;\n}\n",
    &["CPlayer", "SData", "EDir"], false,
)]
#[case::colon_inside_string_literal_does_not_fire(
    "class CPlayer {}\nfunction SomeFunc(\"test:$0\n",
    &[], true,
)]
#[case::no_type_context_outside_annotation(
    "function Test() {\n  some$0Var\n}\n",
    &[], true,
)]
#[case::cursor_right_of_complete_type_name(
    "class CMyType {}\nfunction F() {\n  var z:CMyType$0;\n  var w : int;\n}\n",
    &[], false,
)]
#[case::cursor_inside_final_type_in_error_recovery(
    "class CMyType {}\nfunction F() {\n  var z : A : B : $0CMyType;\n  var w : int;\n}\n",
    &[], false,
)]
#[case::cursor_right_of_final_type_in_error_recovery(
    "class CMyType {}\nfunction F() {\n  var z : A : B : CMyType$0;\n  var w : int;\n}\n",
    &[], false,
)]
fn type_completions_at_cursor(
    #[case] fixture: &str,
    #[case] required: &[&str],
    #[case] must_be_empty: bool,
) {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    let types = type_completions(t.primary_doc(), &t.db(), pos);
    if must_be_empty {
        assert!(
            types.is_empty(),
            "expected empty, got {:?}",
            def_names(&types)
        );
    } else {
        assert!(!types.is_empty(), "expected some completions");
        let names = def_names(&types);
        for r in required {
            assert!(names.contains(r), "expected {r:?} in {names:?}");
        }
    }
}

#[rstest]
#[case::field_type_at_class_body_offers_class_name(3, 15, true, &["CRefType"])]
#[case::field_name_between_methods_offers_nothing(5, 6, false, &[])]
#[case::field_type_between_methods_fires(5, 16, true, &[])]
fn declaration_context_completions(
    #[case] line: u32,
    #[case] character: u32,
    #[case] must_have_types: bool,
    #[case] type_required: &[&str],
) {
    let source = include_str!("../../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let t = TestDb::new(source);
    let pos = SourcePosition { line, character };

    let members = completion_members(t.primary_uri(), t.primary_doc(), &t.db(), pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire here"
    );

    let types = type_completions(t.primary_doc(), &t.db(), pos);
    if must_have_types {
        assert!(!types.is_empty(), "type completions must fire here");
        let names = def_names(&types);
        for n in type_required {
            assert!(names.contains(n), "expected {n:?} in {names:?}");
        }
    } else {
        assert!(types.is_empty(), "type completions must not fire here");
    }
}

#[rstest]
#[case::parameter_type("class CParam {}\nfunction Foo(x : C$0Param) {}\n", "CParam")]
#[case::function_return_type(
    "class CReturnType {}\nfunction Foo() : C$0ReturnType {}\n",
    "CReturnType"
)]
fn type_annotation_in_callable_header_fires_type_completions(
    #[case] fixture: &str,
    #[case] expected: &str,
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let members = completion_members(&uri, t.doc_for(&uri), &t.db(), pos);
    assert!(members.is_empty());
    let types = type_completions(t.doc_for(&uri), &t.db(), pos);
    let names = def_names(&types);
    assert!(
        names.contains(&expected),
        "expected {expected:?} in {names:?}"
    );
}

#[rstest]
#[case::after_extends_keyword(
    "class CExample {}\nclass Foo extends $0\n",
    &["CExample"], &[],
)]
#[case::mid_base_class_name(
    "class CExample {}\nclass Foo extends CEx$0\n",
    &["CExample"], &[],
)]
#[case::inside_class_body_blank(
    "class CExample {}\nclass Foo extends CExample {\n  $0\n}\n",
    &[], &[],
)]
#[case::at_class_name_position(
    "class F$0oo {\n  \n}\n",
    &[], &[],
)]
#[case::state_extends_offers_states_only(
    "class CBase {}\nstate BaseState in CBase {}\nstate IdleState in CBase extends $0\n",
    &["BaseState"], &["CBase"],
)]
#[case::state_extends_excludes_unrelated_owners(
    "class CPlayer {}\nclass CNpc {}\nstate PlayerIdle in CPlayer {}\nstate NpcIdle in CNpc {}\nstate Foo in CPlayer extends $0\n",
    &["PlayerIdle"], &["NpcIdle"],
)]
#[case::state_extends_includes_superclass_states(
    "class CRoot {}\nclass CMid extends CRoot {}\nclass CLeaf extends CMid {}\nstate RootIdle in CRoot {}\nstate MidIdle in CMid {}\nstate LeafIdle in CLeaf {}\nstate OtherIdle in CRoot {}\nstate Foo in CLeaf extends $0\n",
    &["LeafIdle", "MidIdle", "RootIdle", "OtherIdle"], &[],
)]
#[case::class_extends_excludes_self(
    "class CExample {}\nclass Foo extends $0\n",
    &["CExample"], &["Foo"],
)]
#[case::state_extends_excludes_self(
    "class CPlayer {}\nstate Sibling in CPlayer {}\nstate Foo in CPlayer extends $0\n",
    &["Sibling"], &["Foo"],
)]
#[case::state_extends_empty_when_owner_unknown(
    "state Foo in CUnknown extends $0\n",
    &[], &[],
)]
#[case::class_extends_excludes_enums_and_structs(
    "class CExample {}\nstruct SExample {}\nenum EExample {}\nclass Foo extends $0\n",
    &["CExample"], &["SExample", "EExample"],
)]
#[case::between_extends_and_class_body(
    "class CExample {}\nclass C extends $0 {}\n",
    &["CExample"], &[],
)]
fn extends_completions_at_cursor(
    #[case] fixture: &str,
    #[case] required: &[&str],
    #[case] excluded: &[&str],
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let result = extends_completions(t.doc_for(&uri), &t.db(), pos);
    let names = def_names(&result);
    if required.is_empty() && excluded.is_empty() {
        assert!(names.is_empty(), "expected empty, got {names:?}");
    } else {
        for n in required {
            assert!(names.contains(n), "expected {n:?} in {names:?}");
        }
        for n in excluded {
            assert!(!names.contains(n), "excluded {n:?} appeared in {names:?}");
        }
    }
}

#[rstest]
#[case::after_class_name("class Foo $0\n", &["extends"])]
#[case::after_state_name("state Foo $0\n", &["in"])]
#[case::after_state_owner("state Foo in Bar $0\n", &["extends"])]
#[case::inside_class_body("class Foo {\n  $0\n}\n", &[])]
#[case::top_level_blank("$0\n", &[])]
#[case::after_extends_typed("class Foo extends $0\n", &[])]
fn class_header_keywords(#[case] fixture: &str, #[case] expected: &[&str]) {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    let result = class_header_keyword_completions(t.primary_doc(), pos);
    let expected_vec: Vec<&str> = expected.to_vec();
    assert_eq!(result, expected_vec);
}

#[rstest]
#[case::offers_classes_only(
    "class COwner {}\nstate SBase in COwner {}\nstruct SStruct {}\nenum EEnum {}\nstate Foo in $0\n",
    &["COwner"], &["SBase", "SStruct", "EEnum"],
)]
#[case::empty_after_owner_typed(
    "class COwner {}\nstate Foo in COwner $0\n",
    &[], &[],
)]
#[case::empty_in_class_extends_slot(
    "class CBase {}\nclass Foo extends $0\n",
    &[], &[],
)]
fn state_owner_completions_at_cursor(
    #[case] fixture: &str,
    #[case] required: &[&str],
    #[case] excluded: &[&str],
) {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    let result = state_owner_completions(t.primary_doc(), &t.db(), pos);
    let names = def_names(&result);
    if required.is_empty() && excluded.is_empty() {
        assert!(result.is_empty(), "expected empty, got {names:?}");
    } else {
        for n in required {
            assert!(names.contains(n), "expected {n:?} in {names:?}");
        }
        for n in excluded {
            assert!(!names.contains(n), "excluded {n:?} appeared in {names:?}");
        }
    }
}
