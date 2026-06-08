use rstest::rstest;

use crate::document::parse_document;
use crate::resolve::{WorkspaceIndex, overridden_top_level};
use crate::symbols::SymbolKind;

fn base_index(uri: &str, source: &str) -> WorkspaceIndex {
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document(uri, &doc);
    index
}

#[rstest]
#[case::function("function", "function Foo() {}\n", "function Foo() {}\n", true)]
#[case::class("class", "class Foo {}\n", "class Foo {}\n", true)]
#[case::structure("struct", "struct Foo {}\n", "struct Foo {}\n", true)]
#[case::enumeration("enum", "enum Foo { A }\n", "enum Foo { A }\n", true)]
#[case::no_name_collision("no collision", "function Foo() {}\n", "function Bar() {}\n", false)]
fn detects_top_level_override(
    #[case] name: &str,
    #[case] base_src: &str,
    #[case] mod_src: &str,
    #[case] expect_override: bool,
) {
    let base = base_index("file:///game/foo.ws", base_src);
    let mod_doc = parse_document(mod_src).expect("parse should succeed");
    let found = overridden_top_level(mod_doc.symbols.all(), &base);
    assert_eq!(
        !found.is_empty(),
        expect_override,
        "case {name}: override detection mismatch"
    );
}

#[test]
fn ignores_members_that_are_not_top_level() {
    let base = base_index("file:///game/foo.ws", "class C { function Shared() {} }\n");
    let mod_doc = parse_document("class D { function Shared() {} }\n").expect("parse");
    let found = overridden_top_level(mod_doc.symbols.all(), &base);
    assert!(
        found.is_empty(),
        "method `Shared` is a member, not a top-level override"
    );
}

#[test]
fn prefers_base_match_of_same_kind() {
    let mut base = WorkspaceIndex::default();
    base.update_document(
        "file:///game/a.ws",
        &parse_document("function Name() {}\n").expect("parse"),
    );
    base.update_document(
        "file:///game/b.ws",
        &parse_document("class Name {}\n").expect("parse"),
    );
    let mod_doc = parse_document("class Name {}\n").expect("parse");
    let found = overridden_top_level(mod_doc.symbols.all(), &base);
    assert_eq!(found.len(), 1, "single override expected");
    assert_eq!(
        found[0].base.symbol.kind,
        SymbolKind::Class,
        "should match the base symbol of the same kind"
    );
    assert_eq!(
        found[0].base.uri, "file:///game/b.ws",
        "should point at the class declaration's file"
    );
}

#[test]
fn lens_anchor_is_local_declaration() {
    let base = base_index("file:///game/foo.ws", "function Foo() {}\n");
    let mod_doc = parse_document("function Foo() {}\n").expect("parse");
    let local = mod_doc
        .symbols
        .all()
        .iter()
        .find(|s| s.name == "Foo")
        .expect("local Foo");
    let found = overridden_top_level(mod_doc.symbols.all(), &base);
    assert_eq!(
        found[0].range, local.selection_range,
        "anchor should be the overriding declaration's selection range"
    );
}
