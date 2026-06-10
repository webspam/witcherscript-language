use super::{KIND_NOT_CLASS, KIND_NOT_STATEMACHINE, collect_state_owner_diagnostics};
use crate::diagnostics::Severity;
use crate::test_support::TestDb;

#[test]
fn warns_when_owner_class_lacks_statemachine() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Banana {}\n",
        "//- /b.ws\n",
        "state BananaState in Banana {}\n",
    ));

    let result = collect_state_owner_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "exactly one state-owner warning");
    assert_eq!(diags[0].kind, KIND_NOT_STATEMACHINE, "warning kind");
    assert_eq!(diags[0].severity, Severity::Warning, "warning severity");
    assert_eq!(
        diags[0].message,
        "'Banana' is not a state machine, so it cannot host a state. \
         Did you forget the 'statemachine' keyword?",
        "warning message names the owner and the keyword",
    );
    assert_eq!(diags[0].related.len(), 1, "one related location");
    assert_eq!(
        diags[0].related[0].uri, "file:///a.ws",
        "related points at the owner's file",
    );
}

#[test]
fn no_warning_when_owner_is_statemachine() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "statemachine class Banana {}\n",
        "//- /b.ws\n",
        "state BananaState in Banana {}\n",
    ));

    assert!(
        collect_state_owner_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "a state on a statemachine class is valid",
    );
}

#[test]
fn nothing_when_owner_unknown() {
    let t = TestDb::new("state BananaState in Banana {}\n");

    assert!(
        collect_state_owner_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "an unresolved owner is the unknown_type rule's concern, not this one",
    );
}

#[test]
fn warns_even_when_owner_extends_a_statemachine() {
    // statemachine is not inherited: the literal owner must carry the keyword.
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "statemachine class Base {}\n",
        "class Derived extends Base {}\n",
        "//- /b.ws\n",
        "state S in Derived {}\n",
    ));

    let result = collect_state_owner_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "subclass of a statemachine still warns");
    assert_eq!(
        diags[0].kind, KIND_NOT_STATEMACHINE,
        "warning kind, not error"
    );
}

#[test]
fn resolves_owner_declared_in_base_script() {
    let t = TestDb::new("state S in VanillaThing {}\n")
        .with_base_doc("file:///base/thing.ws", "class VanillaThing {}\n");

    let result = collect_state_owner_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "a non-statemachine base-script owner warns");
    assert_eq!(
        diags[0].related[0].uri, "file:///base/thing.ws",
        "related points at the base script",
    );
}

#[test]
fn no_warning_for_statemachine_owner_in_base_script() {
    let t = TestDb::new("state S in CR4Player {}\n").with_base_doc(
        "file:///base/player.ws",
        "statemachine class CR4Player {}\n",
    );

    assert!(
        collect_state_owner_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "a vanilla statemachine owner is valid",
    );
}

#[test]
fn errors_when_owner_is_a_struct() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "struct Banana {}\n",
        "//- /b.ws\n",
        "state BananaState in Banana {}\n",
    ));

    let result = collect_state_owner_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "a struct owner is an error");
    assert_eq!(diags[0].kind, KIND_NOT_CLASS, "error kind");
    assert_eq!(diags[0].severity, Severity::Error, "error severity");
    assert_eq!(
        diags[0].message,
        "'Banana' is not a class; a state can only be declared in a state machine class.",
        "error message names the owner",
    );
    assert_eq!(
        diags[0].related[0].uri, "file:///a.ws",
        "related points at the struct's file",
    );
}

#[test]
fn errors_when_owner_is_an_enum() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "enum Banana { Green, Ripe }\n",
        "//- /b.ws\n",
        "state BananaState in Banana {}\n",
    ));

    let result = collect_state_owner_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "an enum owner is an error");
    assert_eq!(diags[0].kind, KIND_NOT_CLASS, "error kind");
    assert_eq!(diags[0].severity, Severity::Error, "error severity");
}
