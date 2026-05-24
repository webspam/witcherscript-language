use super::collect_unknown_method_diagnostics;
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
    collect_unknown_method_diagnostics(&doc_pairs, &db)
}

#[test]
fn no_diagnostic_for_known_method() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "class Foo { function Bar() {} function Test() { var f : Foo; f.Bar(); } }\n",
    )]);

    let result = check(&idx, &docs);

    assert!(
        result.is_empty(),
        "known method should not produce diagnostic"
    );
}

#[test]
fn no_diagnostic_inherited_method() {
    let (idx, docs) = index_and_docs(&[
        ("file:///a.ws", "class Base { function Inherited() {} }\n"),
        (
            "file:///b.ws",
            "class Child extends Base { function Test() { var c : Child; c.Inherited(); } }\n",
        ),
    ]);

    let result = check(&idx, &docs);

    assert!(
        result.is_empty(),
        "inherited method should not produce diagnostic"
    );
}

#[test]
fn no_diagnostic_this_known_method() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "class Foo { function Bar() {} function Run() { this.Bar(); } }\n",
    )]);

    let result = check(&idx, &docs);

    assert!(
        result.is_empty(),
        "this.method() on known method should not produce diagnostic"
    );
}

#[test]
fn no_diagnostic_unknown_receiver() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "function Test(x : Unknown) { x.Method(); }\n",
    )]);

    let result = check(&idx, &docs);

    assert!(
        result.is_empty(),
        "unknown receiver type should not produce diagnostic"
    );
}

#[test]
fn no_diagnostic_primitive_receiver() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "function Test() { var n : int; n.Method(); }\n",
    )]);

    let result = check(&idx, &docs);

    assert!(
        result.is_empty(),
        "primitive receiver should not produce diagnostic"
    );
}

#[test]
fn flags_unknown_method_on_known_type() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "class Foo { } function Test() { var f : Foo; f.Qux(); }\n",
    )]);

    let result = check(&idx, &docs);

    let diags = result
        .get("file:///test.ws")
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
    assert!(diags[0].message.contains("Qux"));
    assert!(diags[0].message.contains("Foo"));
}

#[test]
fn flags_this_unknown_method() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "class Foo { function Run() { this.Nonexistent(); } }\n",
    )]);

    let result = check(&idx, &docs);

    let diags = result
        .get("file:///test.ws")
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
}

#[test]
fn flags_struct_receiver() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "struct Vec3 { } function Test() { var v : Vec3; v.Normalize(); }\n",
    )]);

    let result = check(&idx, &docs);

    let diags = result
        .get("file:///test.ws")
        .expect("should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
}

#[test]
fn no_false_positive_on_private_method() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "class Foo { private function Secret() {} function Test() { var f : Foo; f.Secret(); } }\n",
    )]);

    let result = check(&idx, &docs);

    assert!(
        result.is_empty(),
        "private method should not produce unknown_method diagnostic"
    );
}

#[test]
fn flags_private_method_call_from_outside_class() {
    let (idx, docs) = index_and_docs(&[(
        "file:///test.ws",
        "class Foo { private function Secret() {} } \
         function Run() { var f : Foo; f.Secret(); }\n",
    )]);

    let result = check(&idx, &docs);

    let diags = result.get("file:///test.ws").unwrap();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "private_member_access");
    assert!(diags[0].message.contains("Secret"));
    assert!(diags[0].message.contains("'Foo'"));
}

#[test]
fn flags_unknown_method_cross_file() {
    let (idx, docs) = index_and_docs(&[
        ("file:///types.ws", "class Widget { function Draw() {} }\n"),
        (
            "file:///use.ws",
            "function Test() { var w : Widget; w.Render(); }\n",
        ),
    ]);

    let result = check(&idx, &docs);

    let diags = result
        .get("file:///use.ws")
        .expect("use.ws should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
    assert!(diags[0].message.contains("Render"));
}

#[test]
fn flags_chained_call_unknown() {
    let (idx, docs) = index_and_docs(&[
        (
            "file:///a.ws",
            "class Builder { function Build() : Result {} }\n",
        ),
        ("file:///b.ws", "class Result { }\n"),
        (
            "file:///c.ws",
            "function Test() { var b : Builder; b.Build().Missing(); }\n",
        ),
    ]);

    let result = check(&idx, &docs);

    let diags = result
        .get("file:///c.ws")
        .expect("c.ws should have diagnostics");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].kind, "unknown_method");
    assert!(diags[0].message.contains("Missing"));
}
