use super::KIND;
use crate::diagnostics::{Severity, WorkspaceDiagnostic, collect_unknown_symbol_diagnostics};
use crate::test_support::TestDb;

fn short_state_name_diagnostics(t: &TestDb, uri: &str) -> Vec<WorkspaceDiagnostic> {
    collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db())
        .remove(uri)
        .unwrap_or_default()
}

#[test]
fn reports_wrap_method_on_short_state_name() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "statemachine class Foo {}\n",
        "state Idle in Foo {}\n",
        "//- /b.ws\n",
        "@wrapMethod(Idle) function Run() { wrappedMethod(); }\n",
    ));

    let diags = short_state_name_diagnostics(&t, "file:///b.ws");

    assert_eq!(diags.len(), 1, "exactly one short-name diagnostic");
    assert_eq!(diags[0].kind, KIND, "diagnostic kind");
    assert_eq!(diags[0].severity, Severity::Error, "error severity");
    assert_eq!(
        diags[0].message,
        "'Idle' is a state's short name, which annotations cannot target; \
         use the state's class name: @wrapMethod(FooStateIdle)",
        "message names the state's class name",
    );
    assert_eq!(diags[0].related.len(), 1, "one related location");
    assert_eq!(
        diags[0].related[0].uri, "file:///a.ws",
        "related points at the state declaration",
    );
}

#[test]
fn accepts_wrap_method_on_state_class_name() {
    let t = TestDb::new(concat!(
        "statemachine class Foo {}\n",
        "state Idle in Foo { function Run() {} }\n",
        "@wrapMethod(FooStateIdle) function Run() { wrappedMethod(); }\n",
    ));

    assert!(
        short_state_name_diagnostics(&t, t.primary_uri()).is_empty(),
        "the state's class name is the valid annotation target",
    );
}

#[test]
fn reports_short_name_of_state_declared_in_base_script() {
    let t = TestDb::new("@addMethod(Swimming) function Extra() {}\n").with_base_doc(
        "file:///base/player.ws",
        "statemachine class CR4Player {}\nstate Swimming in CR4Player {}\n",
    );

    let diags = short_state_name_diagnostics(&t, t.primary_uri());

    assert_eq!(diags.len(), 1, "base-script state short name is flagged");
    assert_eq!(diags[0].kind, KIND, "diagnostic kind");
    assert_eq!(
        diags[0].related[0].uri, "file:///base/player.ws",
        "related points at the base script",
    );
}

#[test]
fn suggests_every_owner_when_short_name_is_shared() {
    let t = TestDb::new(concat!(
        "statemachine class W3PlayerWitcher {}\n",
        "statemachine class W3ReplacerCiri {}\n",
        "state CombatSteel in W3PlayerWitcher {}\n",
        "state CombatSteel in W3ReplacerCiri {}\n",
        "@addMethod(CombatSteel) function Extra() {}\n",
    ));

    let diags = short_state_name_diagnostics(&t, t.primary_uri());

    assert_eq!(
        diags.len(),
        1,
        "one diagnostic for the ambiguous short name"
    );
    assert_eq!(
        diags[0].message,
        "'CombatSteel' is a state's short name, which annotations cannot target; \
         use the state's class name: @addMethod(W3PlayerWitcherStateCombatSteel), \
         @addMethod(W3ReplacerCiriStateCombatSteel)",
        "both owners' class names are suggested",
    );
    assert_eq!(diags[0].related.len(), 2, "one related location per owner");
}

#[test]
fn reports_short_name_shared_with_a_function() {
    let t = TestDb::new(concat!(
        "function Idle() {}\n",
        "statemachine class Foo {}\n",
        "state Idle in Foo {}\n",
        "@addMethod(Idle) function Extra() {}\n",
    ));

    let diags = short_state_name_diagnostics(&t, t.primary_uri());

    assert_eq!(
        diags.iter().map(|d| d.kind.as_str()).collect::<Vec<_>>(),
        [KIND],
        "a same-named function is not a type, so the short state name is still reported"
    );
}

#[test]
fn ignores_annotation_arg_that_is_a_plain_class() {
    let t = TestDb::new(concat!(
        "class Helper {}\n",
        "@addMethod(Helper) function Extra() {}\n",
    ));

    assert!(
        short_state_name_diagnostics(&t, t.primary_uri()).is_empty(),
        "a plain class target is not this rule's concern",
    );
}

#[test]
fn ignores_short_name_shadowed_by_a_class() {
    let t = TestDb::new(concat!(
        "class Idle {}\n",
        "statemachine class Foo {}\n",
        "state Idle in Foo {}\n",
        "@addMethod(Idle) function Extra() {}\n",
    ));

    assert!(
        short_state_name_diagnostics(&t, t.primary_uri()).is_empty(),
        "a class with the same name is the real target",
    );
}
