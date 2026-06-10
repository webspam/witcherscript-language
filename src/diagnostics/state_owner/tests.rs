use super::{KIND, collect_state_owner_diagnostics};
use crate::test_support::TestDb;

#[test]
fn warns_when_owner_class_lacks_statemachine() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Banana {}\n",
        "//- /b.ws\n",
        "state BananaState in Banana {}\n",
    ));

    let result = collect_state_owner_diagnostics(&t.workspace, &t.base);

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "exactly one state-owner warning");
    assert_eq!(diags[0].kind, KIND);
    assert_eq!(
        diags[0].message,
        "State 'BananaState' targets 'Banana', which is not a state machine. \
         Did you forget the 'statemachine' keyword on the class?"
    );
    assert_eq!(diags[0].related.len(), 1, "one related location");
    assert_eq!(diags[0].related[0].uri, "file:///a.ws");
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
        collect_state_owner_diagnostics(&t.workspace, &t.base).is_empty(),
        "a state on a statemachine class is valid"
    );
}

#[test]
fn no_warning_when_owner_unknown() {
    let t = TestDb::new("state BananaState in Banana {}\n");

    assert!(
        collect_state_owner_diagnostics(&t.workspace, &t.base).is_empty(),
        "an unresolved owner is the unknown_type rule's concern, not this one"
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

    let result = collect_state_owner_diagnostics(&t.workspace, &t.base);

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "subclass of a statemachine still warns");
}

#[test]
fn resolves_owner_declared_in_base_script() {
    let t = TestDb::new("state S in VanillaThing {}\n")
        .with_base_doc("file:///base/thing.ws", "class VanillaThing {}\n");

    let result = collect_state_owner_diagnostics(&t.workspace, &t.base);

    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "a non-statemachine base-script owner warns");
    assert_eq!(diags[0].related[0].uri, "file:///base/thing.ws");
}

#[test]
fn no_warning_for_statemachine_owner_in_base_script() {
    let t = TestDb::new("state S in CR4Player {}\n").with_base_doc(
        "file:///base/player.ws",
        "statemachine class CR4Player {}\n",
    );

    assert!(
        collect_state_owner_diagnostics(&t.workspace, &t.base).is_empty(),
        "a vanilla statemachine owner is valid"
    );
}

#[test]
fn no_warning_when_owner_is_a_struct() {
    // Out of scope: a non-class owner is an error reported by another rule.
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "struct Banana {}\n",
        "//- /b.ws\n",
        "state BananaState in Banana {}\n",
    ));

    assert!(
        collect_state_owner_diagnostics(&t.workspace, &t.base).is_empty(),
        "a struct owner is not this rule's concern"
    );
}
