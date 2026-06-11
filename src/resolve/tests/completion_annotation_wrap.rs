use rstest::rstest;

use super::super::{OverrideCompletion, override_completions};
use crate::test_support::{TestDb, def_names};

fn run(fixture: &str) -> Option<OverrideCompletion> {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    override_completions(t.doc_for(&uri), &t.db(), pos)
}

#[test]
fn after_wrap_method_offers_methods_with_function_keyword() {
    let result = run(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@wrapMethod(CPlayer) $0\n",
    ))
    .expect("methods should be offered directly after the annotation");

    assert!(
        result.needs_function_keyword,
        "insert must lead with `function` before the keyword is typed"
    );
    assert!(
        def_names(&result.methods).contains(&"OnSpawned"),
        "method should be offered immediately"
    );
}

#[test]
fn after_wrap_method_none_for_unknown_class() {
    assert!(
        run("@wrapMethod(CUnknown) $0\n").is_none(),
        "unknown class should yield None"
    );
}

#[test]
fn after_wrap_method_none_for_struct() {
    assert!(
        run(concat!("struct SData {}\n", "@wrapMethod(SData) $0\n")).is_none(),
        "struct target should yield None"
    );
}

#[test]
fn after_wrap_method_offers_methods_after_function_keyword() {
    let result = run(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "  public event OnDeath() {}\n",
        "  public var mHp : int;\n",
        "}\n",
        "@wrapMethod(CPlayer)\n",
        "function $0\n",
    ))
    .expect("methods should be offered after `function`");

    assert!(
        !result.needs_function_keyword,
        "`function` already typed; insert must not repeat it"
    );
    let names = def_names(&result.methods);
    assert!(names.contains(&"OnSpawned"), "method should be offered");
    assert!(names.contains(&"OnDeath"), "event should be offered");
    assert!(!names.contains(&"mHp"), "field must not be offered");
}

#[test]
fn after_wrap_method_does_not_walk_inheritance() {
    let result = run(concat!(
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
    ))
    .expect("methods should be offered");

    let names = def_names(&result.methods);
    assert!(names.contains(&"OwnMethod"), "own method should appear");
    assert!(
        !names.contains(&"BaseMethod"),
        "inherited method must not appear"
    );
}

#[test]
fn after_wrap_method_offers_methods_while_typing_first_word() {
    let result = run(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@wrapMethod(CPlayer)\n",
        "fun$0\n",
    ))
    .expect("methods should be offered while typing the first word");

    assert!(
        result.needs_function_keyword,
        "first word is not yet `function`; insert must lead with it"
    );
    assert!(
        def_names(&result.methods).contains(&"OnSpawned"),
        "method should be offered"
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
fn after_wrap_method_offers_methods_while_typing_method_name(#[case] fixture: &str) {
    let result = run(fixture).expect("methods should be offered");
    let names = def_names(&result.methods);
    assert!(names.contains(&"AddMethodA"), "AddMethodA missing");
    assert!(names.contains(&"AddMethodB"), "AddMethodB missing");
}

#[test]
fn after_wrap_method_still_offers_already_wrapped_method() {
    let result = run(concat!(
        "//- /base.ws\n",
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "  public function OnDeath() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "@wrapMethod(CPlayer)\n",
        "function OnSpawned() { wrappedMethod(); }\n",
        "//- /test.ws\n",
        "@wrapMethod(CPlayer)\n",
        "function $0\n",
    ))
    .expect("methods should be offered");

    let names = def_names(&result.methods);
    assert!(
        names.contains(&"OnSpawned"),
        "already-wrapped method must still be offered"
    );
    assert!(names.contains(&"OnDeath"), "unwrapped method missing");
}

#[test]
fn after_wrap_method_none_when_function_not_preceded_by_wrap_method() {
    assert!(
        run("function $0\n").is_none(),
        "bare `function` should not trigger wrap method completions"
    );
}

#[test]
fn after_replace_method_offers_methods_with_function_keyword() {
    let result = run(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@replaceMethod(CPlayer) $0\n",
    ))
    .expect("methods should be offered directly after the annotation");

    assert!(
        result.needs_function_keyword,
        "insert must lead with `function` before the keyword is typed"
    );
    assert!(
        def_names(&result.methods).contains(&"OnSpawned"),
        "method should be offered immediately"
    );
}

#[test]
fn after_replace_method_offers_methods_after_function_keyword() {
    let result = run(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "  public event OnDeath() {}\n",
        "  public var mHp : int;\n",
        "}\n",
        "@replaceMethod(CPlayer)\n",
        "function $0\n",
    ))
    .expect("methods should be offered after `function`");

    assert!(
        !result.needs_function_keyword,
        "`function` already typed; insert must not repeat it"
    );
    let names = def_names(&result.methods);
    assert!(names.contains(&"OnSpawned"), "method should be offered");
    assert!(names.contains(&"OnDeath"), "event should be offered");
    assert!(!names.contains(&"mHp"), "field must not be offered");
}
