use super::collect_duplicate_local_diagnostics;
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
fn param_and_local_same_name() {
    let idx = index(&[("file:///a.ws", "function F(x : int) {\n  var x : int;\n}\n")]);

    let result = collect_duplicate_local_diagnostics(&idx);

    let a = result.get("file:///a.ws").expect("a.ws flagged");
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|d| d.kind == "duplicate_local"));
    assert!(a.iter().all(|d| d.related.len() == 1));
}

#[test]
fn two_locals_same_name() {
    let idx = index(&[(
        "file:///a.ws",
        "function F() {\n  var x : int;\n  var x : int;\n}\n",
    )]);

    let result = collect_duplicate_local_diagnostics(&idx);

    let a = result.get("file:///a.ws").expect("a.ws flagged");
    assert_eq!(a.len(), 2);
}

#[test]
fn same_name_in_different_functions_independent() {
    let idx = index(&[(
        "file:///a.ws",
        "function F() {\n  var x : int;\n}\nfunction G() {\n  var x : int;\n}\n",
    )]);

    assert!(collect_duplicate_local_diagnostics(&idx).is_empty());
}

#[test]
fn wrap_method_exempt_from_duplicate_local() {
    let idx = index(&[(
        "file:///a.ws",
        "@wrapMethod(CR4Player)\nfunction F(x : int) {\n  var x : int;\n}\n",
    )]);

    assert!(
        collect_duplicate_local_diagnostics(&idx).is_empty(),
        "@wrapMethod must suppress duplicate_local"
    );
}
