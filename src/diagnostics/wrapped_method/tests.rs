use super::collect_wrapped_method_diagnostics;
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
    collect_wrapped_method_diagnostics(&doc_pairs, &db)
}

fn kinds(diags: &[super::WorkspaceDiagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.kind.as_str()).collect()
}

#[test]
fn single_call_passes() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @wrapMethod(Foo) function W() { wrappedMethod(); }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn missing_call_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @wrapMethod(Foo) function W() {}\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["missing_wrapped_method"]);
    assert!(diags[0].message.contains("W"));
}

#[test]
fn duplicate_calls_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @wrapMethod(Foo) function W() { wrappedMethod(); wrappedMethod(); wrappedMethod(); }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(
        kinds(diags),
        vec!["duplicate_wrapped_method", "duplicate_wrapped_method"]
    );
}

#[test]
fn unannotated_function_ignored() {
    let (idx, docs) = index_and_docs(&[("file:///t.ws", "function F() { wrappedMethod(); }\n")]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn add_method_annotation_ignored() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @addMethod(Foo) function A() {}\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn wrap_method_with_call_inside_if_passes() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @wrapMethod(Foo) function W() { if (true) { wrappedMethod(); } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn member_access_does_not_count() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @wrapMethod(Foo) function W() { this.wrappedMethod(); }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["missing_wrapped_method"]);
}
