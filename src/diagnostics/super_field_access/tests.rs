use super::collect_super_field_access_diagnostics;
use crate::document::{parse_document, ParsedDocument};
use crate::resolve::{SymbolDb, WorkspaceIndex};

fn index_and_docs(docs: &[(&str, &str)]) -> (WorkspaceIndex, Vec<(String, ParsedDocument)>) {
    let mut idx = WorkspaceIndex::default();
    let mut parsed = Vec::new();
    for (uri, src) in docs {
        let doc = parse_document(*src).expect("parse should succeed");
        idx.update_document(*uri, &doc);
        parsed.push((uri.to_string(), doc));
    }
    (idx, parsed)
}

fn check(
    idx: &WorkspaceIndex,
    docs: &[(String, ParsedDocument)],
) -> std::collections::HashMap<String, Vec<super::WorkspaceDiagnostic>> {
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(idx, &base);
    let doc_pairs: Vec<(&str, &ParsedDocument)> =
        docs.iter().map(|(uri, doc)| (uri.as_str(), doc)).collect();
    collect_super_field_access_diagnostics(&doc_pairs, &db)
}

#[test]
fn flags_super_field_read() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Base { var x : int; } \
         class Derived extends Base { function F() { var y : int; y = super.x; } }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").expect("expected diagnostic");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "super_field_access");
}

#[test]
fn flags_super_field_assignment_target() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Base { var x : int; } \
         class Derived extends Base { function F() { super.x = 1; } }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").expect("expected diagnostic");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "super_field_access");
}

#[test]
fn allows_super_method_call() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Base { function M() {} } \
         class Derived extends Base { function F() { super.M(); } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn allows_this_field_access() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Base { var x : int; } \
         class Derived extends Base { function F() { var y : int; y = this.x; } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn does_not_fire_inside_error_subtree() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Base { var x : int; } \
         class Derived extends Base { function F() { y = super. } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(
        result.is_empty(),
        "expected no super_field_access diagnostics inside error subtree, got {result:?}"
    );
}
