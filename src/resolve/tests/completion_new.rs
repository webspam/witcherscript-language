use rstest::rstest;

use super::super::completion::{new_lifetime_completions, new_type_completions};
use crate::test_support::{def_names, TestDb};

// `new |` class slot. When the expected type is known, narrow to that type and
// its subclasses; otherwise (no LHS, unknown LHS, or a universal base like
// CObject) offer every class.

#[rstest]
#[case::narrowed_to_explicit_base_and_descendants(
    "class CBase {}\nclass CDerived extends CBase {}\nclass CUnrelated {}\nfunction F() { var x : CBase = new $0; }\n",
    &["CBase", "CDerived"], &["CUnrelated"],
)]
#[case::partial_class_name_after_new(
    "class CBase {}\nclass CDerived extends CBase {}\nclass CUnrelated {}\nfunction F() { var x : CBase = new C$0; }\n",
    &["CBase", "CDerived"], &["CUnrelated"],
)]
#[case::cobject_lhs_offers_every_class(
    "class CDerived {}\nclass CUnrelated {}\nfunction F() { var x : CObject = new $0; }\n",
    &["CDerived", "CUnrelated"], &[],
)]
#[case::no_expected_type_offers_all_classes(
    "class CBase {}\nclass CDerived extends CBase {}\nstruct SThing {}\nfunction F() { new $0; }\n",
    &["CBase", "CDerived"], &["SThing"],
)]
#[case::expected_type_unknown_offers_all_classes(
    "class CKnown {}\nfunction F() { var x : CMissing = new $0; }\n",
    &["CKnown"], &[],
)]
#[case::field_assignment_uses_field_type(
    "class CBase {}\nclass CDerived extends CBase {}\nclass CUnrelated {}\nclass CHolder { var slot : CBase; }\nfunction F() { var h : CHolder; h.slot = new $0; }\n",
    &["CBase", "CDerived"], &["CUnrelated"],
)]
#[case::inside_call_arg_does_not_inherit_outer_lhs(
    "class CBase {}\nclass CDerived extends CBase {}\nclass CUnrelated {}\nfunction Take(p : int) : CBase {}\nfunction F() { var x : CBase = Take(new $0); }\n",
    &["CBase", "CDerived", "CUnrelated"], &[],
)]
#[case::not_in_new_position_returns_empty(
    "class C {}\nfunction F() { var x : C = $0; }\n",
    &[], &[],
)]
fn new_type_completions_at_cursor(
    #[case] fixture: &str,
    #[case] required: &[&str],
    #[case] excluded: &[&str],
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let result = new_type_completions(&uri, t.doc_for(&uri), &t.db(), pos);
    let names = def_names(&result);
    if required.is_empty() && excluded.is_empty() {
        assert!(names.is_empty(), "expected empty, got {names:?}");
        return;
    }
    for n in required {
        assert!(names.contains(n), "expected {n:?} in {names:?}");
    }
    for n in excluded {
        assert!(!names.contains(n), "excluded {n:?} appeared in {names:?}");
    }
}

// `new C in |` lifetime slot. Offers in-scope locals, parameters and members
// whose static type is a class (only class instances can own lifetimes).

#[rstest]
#[case::offers_local_class_var(
    "class CObject {}\nclass CHolder {}\nfunction F() { var owner : CHolder; var x : CObject = new CObject in $0; }\n",
    &["owner"], &[],
)]
#[case::excludes_non_class_typed_locals(
    "class CObject {}\nstruct SThing {}\nfunction F() { var n : int; var s : SThing; var x : CObject = new CObject in $0; }\n",
    &[], &["n", "s"],
)]
#[case::offers_parameters_and_excludes_non_class_field(
    "class CHolder { var data : int; function M(p : CHolder) { var x : CObject = new CObject in $0; } }\nclass CObject {}\n",
    &["p"], &["data"],
)]
#[case::offers_class_typed_field(
    "class CObject {}\nclass COwner {}\nclass CWrapper { var slot : COwner; function M() { var x : CObject = new CObject in $0; } }\n",
    &["slot"], &[],
)]
#[case::excludes_locals_declared_below_cursor(
    "class CObject {}\nclass CHolder {}\nfunction F() { var x : CObject = new CObject in $0; var later : CHolder; }\n",
    &[], &["later"],
)]
#[case::partial_name_after_in_keyword(
    "class CObject {}\nclass CHolder {}\nfunction F() { var owner : CHolder; var x : CObject = new CObject in o$0; }\n",
    &["owner"], &[],
)]
#[case::no_in_keyword_returns_none(
    "class C {}\nfunction F() { var x : C = new C $0; }\n",
    &[], &[],
)]
fn new_lifetime_completions_at_cursor(
    #[case] fixture: &str,
    #[case] required: &[&str],
    #[case] excluded: &[&str],
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let result = new_lifetime_completions(&uri, t.doc_for(&uri), &t.db(), pos);
    let names = def_names(&result);
    if required.is_empty() && excluded.is_empty() {
        assert!(names.is_empty(), "expected empty, got {names:?}");
        return;
    }
    for n in required {
        assert!(names.contains(n), "expected {n:?} in {names:?}");
    }
    for n in excluded {
        assert!(!names.contains(n), "excluded {n:?} appeared in {names:?}");
    }
}
