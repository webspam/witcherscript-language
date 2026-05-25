use super::collect_duplicate_local_diagnostics;
use crate::test_support::TestDb;

#[test]
fn param_and_local_same_name() {
    let t = TestDb::new("function F(x : int) {\n  var x : int;\n}\n");
    let result = collect_duplicate_local_diagnostics(&t.workspace);

    let a = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|d| d.kind == "duplicate_local"));
    assert!(a.iter().all(|d| d.related.len() == 1));
}

#[test]
fn two_locals_same_name() {
    let t = TestDb::new("function F() {\n  var x : int;\n  var x : int;\n}\n");
    let result = collect_duplicate_local_diagnostics(&t.workspace);

    let a = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(a.len(), 2);
}

#[test]
fn same_name_in_different_functions_independent() {
    let t = TestDb::new("function F() {\n  var x : int;\n}\nfunction G() {\n  var x : int;\n}\n");
    assert!(collect_duplicate_local_diagnostics(&t.workspace).is_empty());
}

#[test]
fn wrap_method_exempt_from_duplicate_local() {
    let t = TestDb::new("@wrapMethod(CR4Player)\nfunction F(x : int) {\n  var x : int;\n}\n");
    assert!(
        collect_duplicate_local_diagnostics(&t.workspace).is_empty(),
        "@wrapMethod must suppress duplicate_local"
    );
}
