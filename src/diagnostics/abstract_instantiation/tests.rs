use super::collect_abstract_instantiation_diagnostics;
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
    collect_abstract_instantiation_diagnostics(&doc_pairs, &db)
}

#[test]
fn flags_new_on_abstract_class() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "abstract class Base {} \
         function F() { var b : Base; b = new Base in this; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").expect("expected diagnostic");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "abstract_instantiation");
    assert!(diags[0].message.contains("Base"));
}

#[test]
fn allows_new_on_concrete_class() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Concrete {} \
         function F() { var c : Concrete; c = new Concrete in this; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[test]
fn flags_across_files() {
    let (idx, docs) = index_and_docs(&[
        ("file:///a.ws", "abstract class Base {}\n"),
        (
            "file:///b.ws",
            "function F() { var b : Base; b = new Base in this; }\n",
        ),
    ]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///b.ws").expect("expected diagnostic");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "abstract_instantiation");
}

#[test]
fn ignores_unknown_class() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "function F() { var x : Missing; x = new Missing in this; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}
