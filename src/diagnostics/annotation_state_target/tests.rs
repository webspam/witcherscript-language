use super::{KIND, collect_annotation_state_target_diagnostics};
use crate::diagnostics::Severity;
use crate::test_support::TestDb;

#[test]
fn reports_wrap_method_on_backing_class_name() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "statemachine class Foo {}\n",
        "state Idle in Foo {}\n",
        "//- /b.ws\n",
        "@wrapMethod(FooStateIdle) function Run() { wrappedMethod(); }\n",
    ));

    let result = collect_annotation_state_target_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "exactly one backing-class diagnostic");
    assert_eq!(diags[0].kind, KIND, "diagnostic kind");
    assert_eq!(diags[0].severity, Severity::Error, "error severity");
    assert_eq!(
        diags[0].message,
        "'FooStateIdle' is a state's backing class name, which annotations cannot target; \
         use the short state name: @wrapMethod(Idle)",
        "message names the short state name",
    );
    assert_eq!(diags[0].related.len(), 1, "one related location");
    assert_eq!(
        diags[0].related[0].uri, "file:///a.ws",
        "related points at the state declaration",
    );
}

#[test]
fn accepts_wrap_method_on_short_state_name() {
    let t = TestDb::new(concat!(
        "statemachine class Foo {}\n",
        "state Idle in Foo {}\n",
        "@wrapMethod(Idle) function Run() { wrappedMethod(); }\n",
    ));

    assert!(
        collect_annotation_state_target_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "the short state name is the valid annotation target",
    );
}

#[test]
fn reports_backing_class_declared_in_base_script() {
    let t = TestDb::new("@addMethod(CR4PlayerStateSwimming) function Extra() {}\n").with_base_doc(
        "file:///base/player.ws",
        "statemachine class CR4Player {}\nstate Swimming in CR4Player {}\n",
    );

    let result = collect_annotation_state_target_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "base-script backing class is flagged");
    assert_eq!(
        diags[0].related[0].uri, "file:///base/player.ws",
        "related points at the base script",
    );
}

#[test]
fn ignores_annotation_arg_that_is_a_plain_class() {
    let t = TestDb::new(concat!(
        "class Helper {}\n",
        "@addMethod(Helper) function Extra() {}\n",
    ));

    assert!(
        collect_annotation_state_target_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "a plain class target is not this rule's concern",
    );
}
