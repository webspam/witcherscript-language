use rstest::rstest;

use super::super::{after_wrap_method_completions, AfterWrapMethodCompletions};
use crate::test_support::TestDb;

#[test]
fn after_wrap_method_stage1_offers_only_function_keyword() {
    let t = TestDb::new(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@wrapMethod(CPlayer) $0\n",
    ));
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);

    assert!(
        matches!(result, Some(AfterWrapMethodCompletions::FunctionKeyword)),
        "stage 1 must yield FunctionKeyword, got {result:?}"
    );
}

#[test]
fn after_wrap_method_stage1_none_for_unknown_class() {
    let t = TestDb::new("@wrapMethod(CUnknown) $0\n");
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);

    assert!(result.is_none(), "unknown class should yield None");
}

#[test]
fn after_wrap_method_stage1_none_for_struct() {
    let t = TestDb::new(concat!("struct SData {}\n", "@wrapMethod(SData) $0\n"));
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);

    assert!(result.is_none(), "struct target should yield None");
}

#[test]
fn after_wrap_method_stage2_offers_method_list_after_function_keyword() {
    let t = TestDb::new(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "  public event OnDeath() {}\n",
        "  public var mHp : int;\n",
        "}\n",
        "@wrapMethod(CPlayer)\n",
        "function $0\n",
    ));
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);

    let methods = match result {
        Some(AfterWrapMethodCompletions::MethodList(m)) => m,
        other => panic!("expected MethodList, got {other:?}"),
    };
    let names: Vec<&str> = methods.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains(&"OnSpawned"), "method should be offered");
    assert!(names.contains(&"OnDeath"), "event should be offered");
    assert!(!names.contains(&"mHp"), "field must not be offered");
}

#[test]
fn after_wrap_method_stage2_does_not_walk_inheritance() {
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CBase {\n",
        "  public function BaseMethod() {}\n",
        "}\n",
        "//- /test.ws\n",
        "class CChild extends CBase {\n",
        "  public function OwnMethod() {}\n",
        "}\n",
        "@wrapMethod(CChild)\n",
        "function $0\n",
    ));
    let (uri, pos) = t.cursor();
    let methods = match after_wrap_method_completions(t.doc_for(&uri), &t.db(), pos) {
        Some(AfterWrapMethodCompletions::MethodList(m)) => m,
        other => panic!("expected MethodList, got {other:?}"),
    };

    let names: Vec<&str> = methods.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains(&"OwnMethod"), "own method should appear");
    assert!(
        !names.contains(&"BaseMethod"),
        "inherited method must not appear"
    );
}

#[test]
fn after_wrap_method_stage1_offers_function_keyword_while_typing_partial_word() {
    let t = TestDb::new(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@wrapMethod(CPlayer)\n",
        "fun$0\n",
    ));
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);

    assert!(
        matches!(result, Some(AfterWrapMethodCompletions::FunctionKeyword)),
        "typing a partial first word after @wrapMethod must yield FunctionKeyword, got {result:?}"
    );
}

#[rstest]
#[case::after_first_letter(concat!(
    "class CPlayer {\n",
    "  public function AddMethodA() {}\n",
    "  public function AddMethodB() {}\n",
    "}\n",
    "@wrapMethod(CPlayer) function A$0ddMet\n",
))]
#[case::midway_through(concat!(
    "class CPlayer {\n",
    "  public function AddMethodA() {}\n",
    "  public function AddMethodB() {}\n",
    "}\n",
    "@wrapMethod(CPlayer) function Add$0Met\n",
))]
#[case::end_of_partial(concat!(
    "class CPlayer {\n",
    "  public function AddMethodA() {}\n",
    "  public function AddMethodB() {}\n",
    "}\n",
    "@wrapMethod(CPlayer) function AddMet$0\n",
))]
fn after_wrap_method_stage2_offers_method_list_while_typing_method_name(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);
    let methods = match result {
        Some(AfterWrapMethodCompletions::MethodList(m)) => m,
        other => panic!("expected MethodList, got {other:?}"),
    };
    let names: Vec<&str> = methods.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains(&"AddMethodA"), "AddMethodA missing");
    assert!(names.contains(&"AddMethodB"), "AddMethodB missing");
}

#[test]
fn after_wrap_method_stage2_none_when_function_not_preceded_by_wrap_method() {
    let t = TestDb::new("function $0\n");
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);

    assert!(
        result.is_none(),
        "bare `function` should not trigger wrap method completions"
    );
}

#[test]
fn after_replace_method_stage1_offers_only_function_keyword() {
    let t = TestDb::new(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@replaceMethod(CPlayer) $0\n",
    ));
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);

    assert!(
        matches!(result, Some(AfterWrapMethodCompletions::FunctionKeyword)),
        "stage 1 must yield FunctionKeyword for @replaceMethod, got {result:?}"
    );
}

#[test]
fn after_replace_method_stage2_offers_method_list() {
    let t = TestDb::new(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "  public event OnDeath() {}\n",
        "  public var mHp : int;\n",
        "}\n",
        "@replaceMethod(CPlayer)\n",
        "function $0\n",
    ));
    let (_uri, pos) = t.cursor();
    let result = after_wrap_method_completions(t.primary_doc(), &t.db(), pos);

    let methods = match result {
        Some(AfterWrapMethodCompletions::MethodList(m)) => m,
        other => panic!("expected MethodList, got {other:?}"),
    };
    let names: Vec<&str> = methods.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains(&"OnSpawned"), "method should be offered");
    assert!(names.contains(&"OnDeath"), "event should be offered");
    assert!(!names.contains(&"mHp"), "field must not be offered");
}
