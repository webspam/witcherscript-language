use super::{make_doc, make_index};
use crate::symbols::SymbolKind;

#[test]
fn all_top_level_yields_top_level_symbols_across_documents() {
    let doc_a = make_doc("class Foo {\n  function Method() {}\n  var field : int;\n}\n");
    let doc_b = make_doc("function Bar() {}\n");
    let mut index = make_index("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let mut found: Vec<(String, String)> = index
        .all_top_level()
        .map(|(uri, sym)| (uri.to_string(), sym.name.clone()))
        .collect();
    found.sort();

    assert_eq!(
        found,
        vec![
            ("file:///a.ws".to_string(), "Foo".to_string()),
            ("file:///b.ws".to_string(), "Bar".to_string()),
        ]
    );
}

#[test]
fn all_top_level_excludes_members() {
    let doc = make_doc("class Foo {\n  function Method() {}\n  var field : int;\n}\n");
    let index = make_index("file:///a.ws", &doc);

    assert!(
        index
            .all_top_level()
            .all(|(_, sym)| sym.container.is_none()),
        "all_top_level must not yield members"
    );
    assert!(
        index
            .all_top_level()
            .all(|(_, sym)| sym.kind != SymbolKind::Method && sym.kind != SymbolKind::Field),
        "all_top_level must not yield methods or fields"
    );
}

#[test]
fn generation_counter_bumps_on_mutation() {
    let doc_a = make_doc("class Foo {}\n");
    let doc_b = make_doc("class Bar {}\n");
    let mut index = super::WorkspaceIndex::default();
    let g0 = index.generation();

    index.update_document("file:///a.ws", &doc_a);
    let g1 = index.generation();
    assert_ne!(g0, g1, "update_document should bump generation");

    index.update_document("file:///b.ws", &doc_b);
    let g2 = index.generation();
    assert_ne!(g1, g2, "second update should bump again");

    index.remove_document("file:///b.ws");
    let g3 = index.generation();
    assert_ne!(g2, g3, "remove_document should bump generation");
}

#[test]
fn surface_hash_stable_under_text_only_edits() {
    let original = make_doc("class A { function F() { var x : int; } }\n");
    let with_comment = make_doc("class A { function F() { var x : int; /* hi */ } }\n");
    let with_body_change = make_doc("class A { function F() { var x : int; x = 42; } }\n");

    let mut index = super::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &original);
    let h0 = index.surface_hash();

    index.update_document("file:///a.ws", &with_comment);
    assert_eq!(
        h0,
        index.surface_hash(),
        "comment-only edit must not change surface hash"
    );

    index.update_document("file:///a.ws", &with_body_change);
    assert_eq!(
        h0,
        index.surface_hash(),
        "function-body edit must not change surface hash"
    );
}

#[test]
fn surface_hash_ignores_local_variable_and_parameter_churn() {
    let baseline = make_doc("class A { function F() { var x : int; } }\n");
    let mut index = super::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &baseline);
    let h0 = index.surface_hash();

    let mid_typing_locals = [
        "class A { function F() { var x : int; var y : float; } }\n",
        "class A { function F() { var x : int; var y : float; var z : string; } }\n",
        "class A { function F() { var renamed : int; } }\n",
        "class A { function F() {} }\n",
    ];
    for src in mid_typing_locals {
        index.update_document("file:///a.ws", &make_doc(src));
        assert_eq!(
            h0,
            index.surface_hash(),
            "local-var churn should not change surface hash: {src}"
        );
    }
}

#[test]
fn surface_hash_changes_on_structural_edit() {
    struct Case {
        name: &'static str,
        before: &'static str,
        after: &'static str,
    }
    let cases = [
        Case {
            name: "rename class",
            before: "class A {}\n",
            after: "class B {}\n",
        },
        Case {
            name: "add method",
            before: "class A { function F() {} }\n",
            after: "class A { function F() {} function G() {} }\n",
        },
        Case {
            name: "change member access",
            before: "class A { public function F() {} }\n",
            after: "class A { private function F() {} }\n",
        },
        Case {
            name: "change base class",
            before: "class A extends Foo {}\n",
            after: "class A extends Bar {}\n",
        },
        Case {
            name: "change parameter signature",
            before: "function F(x : int) {}\n",
            after: "function F(x : float) {}\n",
        },
    ];
    for c in cases {
        let mut index = super::WorkspaceIndex::default();
        index.update_document("file:///x.ws", &make_doc(c.before));
        let h_before = index.surface_hash();
        index.update_document("file:///x.ws", &make_doc(c.after));
        let h_after = index.surface_hash();
        assert_ne!(
            h_before, h_after,
            "case `{}`: surface hash should change",
            c.name
        );
    }
}

#[test]
fn surface_hash_does_not_self_cancel_for_identical_docs() {
    let doc = make_doc("class A {}\n");
    let mut index = super::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc);
    index.update_document("file:///b.ws", &doc);
    assert_ne!(
        0,
        index.surface_hash(),
        "two identical-content docs under different URIs must not XOR to 0"
    );
}

#[test]
fn surface_hash_recovers_after_revert() {
    let original = make_doc("class A {}\n");
    let edited = make_doc("class B {}\n");
    let mut index = super::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &original);
    let h0 = index.surface_hash();
    index.update_document("file:///a.ws", &edited);
    assert_ne!(h0, index.surface_hash());
    index.update_document("file:///a.ws", &original);
    assert_eq!(
        h0,
        index.surface_hash(),
        "reverting should restore prior surface hash"
    );
}

#[test]
fn surface_hash_returns_to_zero_after_removing_only_doc() {
    let doc = make_doc("class A {}\n");
    let mut index = super::WorkspaceIndex::default();
    assert_eq!(0, index.surface_hash());
    index.update_document("file:///a.ws", &doc);
    assert_ne!(0, index.surface_hash());
    index.remove_document("file:///a.ws");
    assert_eq!(
        0,
        index.surface_hash(),
        "removing the only indexed doc should restore empty hash"
    );
}
