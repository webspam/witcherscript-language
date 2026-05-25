use super::collect_duplicate_symbol_diagnostics;
use crate::test_support::TestDb;

#[test]
fn flags_cross_file_class_and_function_conflict() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Foo {}\n",
        "//- /b.ws\n",
        "function Foo() {}\n",
    ));

    let result = collect_duplicate_symbol_diagnostics(&t.workspace);

    let a = result.get("file:///a.ws").expect("a.ws should be flagged");
    let b = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].kind, "duplicate_symbol");
    assert_eq!(
        a[0].message,
        "A class or function with that name already exists."
    );
    assert_eq!(a[0].related.len(), 1);
    assert_eq!(a[0].related[0].uri, "file:///b.ws");
    assert_eq!(b[0].related[0].uri, "file:///a.ws");
}

#[test]
fn flags_same_file_duplicate() {
    let t = TestDb::new("class Foo {}\nclass Foo {}\n");

    let result = collect_duplicate_symbol_diagnostics(&t.workspace);

    let a = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|d| d.related.len() == 1));
}

#[test]
fn no_duplicates_returns_empty() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Foo {}\n",
        "//- /b.ws\n",
        "function Bar() {}\n",
    ));

    assert!(collect_duplicate_symbol_diagnostics(&t.workspace).is_empty());
}

#[test]
fn same_named_states_in_different_statemachines_are_not_duplicates() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "state Combat in CR4Player {}\n",
        "//- /b.ws\n",
        "state Combat in W3MonsterAI {}\n",
    ));

    assert!(
        collect_duplicate_symbol_diagnostics(&t.workspace).is_empty(),
        "states sharing a name across different statemachines are valid WitcherScript"
    );
}

#[test]
fn same_named_states_in_same_statemachine_are_duplicates() {
    let t = TestDb::new("state Combat in CR4Player {}\nstate Combat in CR4Player {}\n");

    let result = collect_duplicate_symbol_diagnostics(&t.workspace);

    let a = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|d| d.related.len() == 1));
}

#[test]
fn state_does_not_collide_with_same_named_class() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Combat {}\n",
        "//- /b.ws\n",
        "state Combat in CR4Player {}\n",
    ));

    assert!(
        collect_duplicate_symbol_diagnostics(&t.workspace).is_empty(),
        "a state is scoped to its statemachine and does not share the global name space"
    );
}

#[test]
fn annotated_member_injection_is_excluded() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Foo {}\n",
        "//- /b.ws\n",
        "@wrapMethod(CR4Player)\nfunction Foo() {}\n",
    ));

    assert!(
        collect_duplicate_symbol_diagnostics(&t.workspace).is_empty(),
        "an @wrapMethod function must not collide with a class name"
    );
}
