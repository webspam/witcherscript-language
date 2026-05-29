use rstest::rstest;

use super::super::{find_references, resolve_definition};
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

#[rstest]
#[case::top_level_function_two_call_sites(
    "function $0Foo() {}\nfunction Bar() {\n Foo();\n Foo();\n}\n",
    false,
    2,
    &[],
)]
#[case::include_decl_true_counts_declaration(
    "function $0Foo() {}\nfunction Bar() {\n Foo();\n}\n",
    true,
    2,
    &[],
)]
#[case::include_decl_false_omits_declaration(
    "function $0Foo() {}\nfunction Bar() {\n Foo();\n}\n",
    false,
    1,
    &[],
)]
#[case::local_variable_scoped_to_function(
    "function Outer() {\n var x : int;\n $0x = 1;\n}\nfunction Other() {\n var x : int;\n}\n",
    true,
    2,
    &[],
)]
#[case::class_body_plus_wrap_plus_call_site(
    "//- /base.ws\n\
     class CPlayer {\n  public function On$0Spawned() {}\n}\n\
     //- /a.ws\n\
     @wrapMethod(CPlayer)\nfunction OnSpawned() {}\n\
     //- /caller.ws\n\
     function Caller() {\n  var p : CPlayer;\n  p.OnSpawned();\n}\n",
    true,
    3,
    &["file:///base.ws", "file:///a.ws", "file:///caller.ws"],
)]
#[case::same_set_from_wrap_function_name(
    "//- /base.ws\n\
     class CPlayer {\n  public function OnSpawned() {}\n}\n\
     //- /a.ws\n\
     @wrapMethod(CPlayer)\nfunction On$0Spawned() {}\n\
     //- /caller.ws\n\
     function Caller() {\n  var p : CPlayer;\n  p.OnSpawned();\n}\n",
    true,
    3,
    &["file:///base.ws", "file:///a.ws"],
)]
#[case::from_wrapped_method_macro(
    "//- /base.ws\n\
     class CPlayer {\n  public function OnSpawned() {}\n}\n\
     //- /a.ws\n\
     @wrapMethod(CPlayer)\nfunction OnSpawned() {\n  wrapped$0Method();\n}\n\
     //- /caller.ws\n\
     function Caller() {\n  var p : CPlayer;\n  p.OnSpawned();\n}\n",
    true,
    3,
    &["file:///base.ws", "file:///a.ws", "file:///caller.ws"],
)]
#[case::exclude_declaration_keeps_only_call_site(
    "//- /base.ws\n\
     class CPlayer {\n  public function On$0Spawned() {}\n}\n\
     //- /a.ws\n\
     @wrapMethod(CPlayer)\nfunction OnSpawned() {}\n\
     //- /caller.ws\n\
     function Caller() {\n  var p : CPlayer;\n  p.OnSpawned();\n}\n",
    false,
    1,
    &["file:///caller.ws"],
)]
fn references(
    #[case] fixture: &str,
    #[case] include_decl: bool,
    #[case] expected_count: usize,
    #[case] required_uris: &[&str],
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let def = resolve_definition(&uri, doc, &t.db(), pos).expect("definition should resolve");
    let search = t.search_docs();
    let refs = find_references(&def, doc, &search, &t.db(), include_decl);
    assert_eq!(
        refs.len(),
        expected_count,
        "actual: {:?}",
        refs.iter().map(|(u, _)| u).collect::<Vec<_>>()
    );
    for required in required_uris {
        assert!(
            refs.iter().any(|(u, _)| u == required),
            "missing required uri {required:?}"
        );
    }
}

#[test]
fn private_member_scope_blocks_homonym_in_other_files() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class A {\n",
        "  private function $0Secret() {}\n",
        "  function Test() {\n",
        "    this.Secret();\n",
        "  }\n",
        "}\n",
        "//- /b.ws\n",
        "function Secret() {}\n",
    ));
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let def = resolve_definition(&uri, doc, &t.db(), pos).expect("private method must resolve");
    assert_eq!(def.symbol.kind, SymbolKind::Method);

    let refs = find_references(&def, doc, &t.search_docs(), &t.db(), false);
    assert_eq!(
        refs.len(),
        1,
        "the top-level Secret() in b.ws must not match"
    );
    assert_eq!(refs[0].0, "file:///a.ws");
}

#[test]
fn private_member_with_wrap_still_searches_other_files() {
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CPlayer {\n",
        "  private function $0Secret() {}\n",
        "}\n",
        "//- /a.ws\n",
        "@wrapMethod(CPlayer)\nfunction Secret() {}\n",
    ));
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let def = resolve_definition(&uri, doc, &t.db(), pos).expect("must resolve");
    assert_eq!(def.symbol.kind, SymbolKind::Method);

    let refs = find_references(&def, doc, &t.search_docs(), &t.db(), true);
    assert!(refs.iter().any(|(u, _)| u == "file:///a.ws"));
    assert!(refs.iter().any(|(u, _)| u == "file:///base.ws"));
}

#[test]
fn addfield_same_name_different_classes_are_independent_symbols() {
    let t = TestDb::new(concat!(
        "@addField(CR4Game)\n",
        "private var $0lightRewriteSettings : CLightRewriteSettings;\n",
        "@addField(CR4IngameMenu)\n",
        "private var lightRewriteSettings : CLightRewriteSettings;\n",
        "class CR4Game {}\n",
        "class CR4IngameMenu {}\n",
        "class CLightRewriteSettings {}\n",
    ));
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let def_game = resolve_definition(&uri, doc, &t.db(), pos).expect("CR4Game field must resolve");
    assert_eq!(def_game.symbol.name, "lightRewriteSettings");

    let def_menu = resolve_definition(
        &uri,
        doc,
        &t.db(),
        crate::line_index::SourcePosition {
            line: 3,
            character: 12,
        },
    )
    .expect("CR4IngameMenu field must resolve");
    assert_eq!(def_menu.symbol.name, "lightRewriteSettings");
    assert_ne!(
        def_game.symbol.selection_byte_range, def_menu.symbol.selection_byte_range,
        "both @addField declarations must be distinct symbols"
    );

    let refs_game = find_references(&def_game, doc, &t.search_docs(), &t.db(), true);
    let refs_menu = find_references(&def_menu, doc, &t.search_docs(), &t.db(), true);
    assert_eq!(refs_game.len(), 1);
    assert_eq!(refs_menu.len(), 1);
}
