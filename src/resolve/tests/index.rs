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
