use super::collect_duplicate_symbol_diagnostics;
use crate::document::parse_document;
use crate::resolve::WorkspaceIndex;

fn index(docs: &[(&str, &str)]) -> WorkspaceIndex {
    let mut idx = WorkspaceIndex::default();
    for (uri, src) in docs {
        let doc = parse_document(*src).expect("parse should succeed");
        idx.update_document(*uri, &doc);
    }
    idx
}

#[test]
fn flags_cross_file_class_and_function_conflict() {
    let idx = index(&[
        ("file:///a.ws", "class Foo {}\n"),
        ("file:///b.ws", "function Foo() {}\n"),
    ]);

    let result = collect_duplicate_symbol_diagnostics(&idx);

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
    let idx = index(&[("file:///a.ws", "class Foo {}\nclass Foo {}\n")]);

    let result = collect_duplicate_symbol_diagnostics(&idx);

    let a = result.get("file:///a.ws").expect("a.ws should be flagged");
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|d| d.related.len() == 1));
}

#[test]
fn no_duplicates_returns_empty() {
    let idx = index(&[
        ("file:///a.ws", "class Foo {}\n"),
        ("file:///b.ws", "function Bar() {}\n"),
    ]);

    assert!(collect_duplicate_symbol_diagnostics(&idx).is_empty());
}

#[test]
fn same_named_states_in_different_statemachines_are_not_duplicates() {
    let idx = index(&[
        ("file:///a.ws", "state Combat in CR4Player {}\n"),
        ("file:///b.ws", "state Combat in W3MonsterAI {}\n"),
    ]);

    assert!(
        collect_duplicate_symbol_diagnostics(&idx).is_empty(),
        "states sharing a name across different statemachines are valid WitcherScript"
    );
}

#[test]
fn same_named_states_in_same_statemachine_are_duplicates() {
    let idx = index(&[(
        "file:///a.ws",
        "state Combat in CR4Player {}\nstate Combat in CR4Player {}\n",
    )]);

    let result = collect_duplicate_symbol_diagnostics(&idx);

    let a = result.get("file:///a.ws").expect("a.ws should be flagged");
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|d| d.related.len() == 1));
}

#[test]
fn state_does_not_collide_with_same_named_class() {
    let idx = index(&[
        ("file:///a.ws", "class Combat {}\n"),
        ("file:///b.ws", "state Combat in CR4Player {}\n"),
    ]);

    assert!(
        collect_duplicate_symbol_diagnostics(&idx).is_empty(),
        "a state is scoped to its statemachine and does not share the global name space"
    );
}

#[test]
fn annotated_member_injection_is_excluded() {
    let idx = index(&[
        ("file:///a.ws", "class Foo {}\n"),
        (
            "file:///b.ws",
            "@wrapMethod(CR4Player)\nfunction Foo() {}\n",
        ),
    ]);

    assert!(
        collect_duplicate_symbol_diagnostics(&idx).is_empty(),
        "an @wrapMethod function must not collide with a class name"
    );
}
