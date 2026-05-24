use super::super::{find_references, resolve_definition};
use super::{index_docs, make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

#[test]
fn finds_references_to_top_level_function() {
    let source = "function Foo() {}\nfunction Bar() {\n Foo();\n Foo();\n}\n";
    let document = make_doc(source);
    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&WorkspaceIndex::default(), &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 9,
        },
    )
    .expect("definition should resolve");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document);

    let refs = find_references(
        &definition,
        &document,
        &[("file:///test.ws", &document)],
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        false,
    );
    assert_eq!(refs.len(), 2, "two call sites expected");
}

#[test]
fn find_references_respects_include_declaration() {
    let source = "function Foo() {}\nfunction Bar() {\n Foo();\n}\n";
    let document = make_doc(source);
    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&WorkspaceIndex::default(), &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 9,
        },
    )
    .expect("definition should resolve");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document);

    let with_decl = find_references(
        &definition,
        &document,
        &[("file:///test.ws", &document)],
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        true,
    );
    let without_decl = find_references(
        &definition,
        &document,
        &[("file:///test.ws", &document)],
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        false,
    );
    assert_eq!(with_decl.len(), 2);
    assert_eq!(without_decl.len(), 1);
}

#[test]
fn finds_references_to_local_variable_within_function_scope() {
    let source =
        "function Outer() {\n var x : int;\n x = 1;\n}\nfunction Other() {\n var x : int;\n}\n";
    let document = make_doc(source);
    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&WorkspaceIndex::default(), &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 1,
        },
    )
    .expect("local variable should resolve");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document);

    let refs = find_references(
        &definition,
        &document,
        &[("file:///test.ws", &document)],
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        true,
    );
    // Should find x in Outer() only: the declaration and the assignment
    assert_eq!(refs.len(), 2, "x in Other() should not be included");
}

#[test]
fn find_references_for_private_member_scoped_to_defining_file() {
    let source_a = concat!(
        "class A {\n",
        "  private function Secret() {}\n",
        "  function Test() {\n",
        "    this.Secret();\n",
        "  }\n",
        "}\n",
    );
    let source_b = "function Secret() {}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Resolve definition of 'Secret' at declaration site (line 1, col 20)
    // "  private function Secret() {}" — 'S' is at col 19
    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &db,
        SourcePosition {
            line: 1,
            character: 20,
        },
    )
    .expect("private method should resolve at definition site");

    assert_eq!(definition.symbol.name, "Secret");
    assert_eq!(definition.symbol.kind, SymbolKind::Method);

    let search_docs = vec![("file:///a.ws", &doc_a), ("file:///b.ws", &doc_b)];
    let refs = find_references(&definition, &doc_a, &search_docs, &db, false);

    // Only the call site in a.ws should appear; the top-level function in b.ws must not
    assert_eq!(refs.len(), 1, "reference in b.ws must not be included");
    assert!(
        refs[0].0 == "file:///a.ws",
        "sole reference must be in the defining file"
    );
}

// --- @addField with same name on different classes are independent symbols ---

#[test]
fn addfield_same_name_different_classes_are_independent() {
    let source = concat!(
        "@addField(CR4Game)\n",
        "private var lightRewriteSettings : CLightRewriteSettings;\n",
        "@addField(CR4IngameMenu)\n",
        "private var lightRewriteSettings : CLightRewriteSettings;\n",
        "class CR4Game {}\n",
        "class CR4IngameMenu {}\n",
        "class CLightRewriteSettings {}\n",
    );
    let document = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def_game = resolve_definition(
        "file:///test.ws",
        &document,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    )
    .expect("CR4Game field should resolve");
    assert_eq!(def_game.symbol.name, "lightRewriteSettings");

    let def_menu = resolve_definition(
        "file:///test.ws",
        &document,
        &db,
        SourcePosition {
            line: 3,
            character: 12,
        },
    )
    .expect("CR4IngameMenu field should resolve");
    assert_eq!(def_menu.symbol.name, "lightRewriteSettings");

    assert_ne!(
        def_game.symbol.selection_byte_range, def_menu.symbol.selection_byte_range,
        "both @addField declarations must not be treated as the same symbol"
    );

    let search_docs = vec![("file:///test.ws", &document)];

    let refs_game = find_references(&def_game, &document, &search_docs, &db, true);
    let refs_menu = find_references(&def_menu, &document, &search_docs, &db, true);

    assert_eq!(
        refs_game.len(),
        1,
        "CR4Game field references must not include CR4IngameMenu's field"
    );
    assert_eq!(
        refs_menu.len(),
        1,
        "CR4IngameMenu field references must not include CR4Game's field"
    );
}

// --- find_references unifies class-body + @wrapMethod/@replaceMethod declarations ---

#[test]
fn find_references_includes_class_body_and_wrap_declarations() {
    let base = make_doc("class CPlayer {\n  public function OnSpawned() {}\n}\n");
    let mod_a = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {}\n");
    let caller = make_doc("function Caller() {\n  var p : CPlayer;\n  p.OnSpawned();\n}\n");
    let index = index_docs(&[
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    // Definition resolved from the class-body declaration site.
    let definition = resolve_definition(
        "file:///base.ws",
        &base,
        &db,
        SourcePosition {
            line: 1,
            character: 21,
        },
    )
    .expect("class-body method should resolve");
    assert_eq!(definition.symbol.kind, SymbolKind::Method);

    let search_docs = vec![
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ];
    let refs = find_references(&definition, &base, &search_docs, &db, true);

    assert_eq!(refs.len(), 3, "two declarations plus one call site");
    assert!(refs.iter().any(|(u, _)| u == "file:///base.ws"));
    assert!(refs.iter().any(|(u, _)| u == "file:///a.ws"));
    assert!(refs.iter().any(|(u, _)| u == "file:///caller.ws"));
}

#[test]
fn find_references_from_wrap_function_name_unifies() {
    let base = make_doc("class CPlayer {\n  public function OnSpawned() {}\n}\n");
    let mod_a = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {}\n");
    let caller = make_doc("function Caller() {\n  var p : CPlayer;\n  p.OnSpawned();\n}\n");
    let index = index_docs(&[
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    // Definition resolved from the @wrapMethod function's own name.
    let definition = resolve_definition(
        "file:///a.ws",
        &mod_a,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    )
    .expect("wrap function name should resolve");

    let search_docs = vec![
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ];
    let refs = find_references(&definition, &base, &search_docs, &db, true);

    assert_eq!(
        refs.len(),
        3,
        "querying from the wrap name yields the same set"
    );
    assert!(refs.iter().any(|(u, _)| u == "file:///base.ws"));
    assert!(refs.iter().any(|(u, _)| u == "file:///a.ws"));
}

#[test]
fn find_references_exclude_declaration_skips_both_decls() {
    let base = make_doc("class CPlayer {\n  public function OnSpawned() {}\n}\n");
    let mod_a = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {}\n");
    let caller = make_doc("function Caller() {\n  var p : CPlayer;\n  p.OnSpawned();\n}\n");
    let index = index_docs(&[
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let definition = resolve_definition(
        "file:///base.ws",
        &base,
        &db,
        SourcePosition {
            line: 1,
            character: 21,
        },
    )
    .expect("class-body method should resolve");

    let search_docs = vec![
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ];
    let refs = find_references(&definition, &base, &search_docs, &db, false);

    assert_eq!(refs.len(), 1, "only the call site, neither declaration");
    assert_eq!(refs[0].0, "file:///caller.ws");
}

#[test]
fn find_references_private_method_with_wrap_searches_all_documents() {
    let base = make_doc("class CPlayer {\n  private function Secret() {}\n}\n");
    let mod_a = make_doc("@wrapMethod(CPlayer)\nfunction Secret() {}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///a.ws", &mod_a)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let definition = resolve_definition(
        "file:///base.ws",
        &base,
        &db,
        SourcePosition {
            line: 1,
            character: 20,
        },
    )
    .expect("private class-body method should resolve");
    assert_eq!(definition.symbol.kind, SymbolKind::Method);

    let search_docs = vec![("file:///base.ws", &base), ("file:///a.ws", &mod_a)];
    let refs = find_references(&definition, &base, &search_docs, &db, true);

    // A wrapped private method must search across files, not just the defining one.
    assert!(
        refs.iter().any(|(u, _)| u == "file:///a.ws"),
        "the @wrapMethod declaration in another file must be found"
    );
    assert!(refs.iter().any(|(u, _)| u == "file:///base.ws"));
}
