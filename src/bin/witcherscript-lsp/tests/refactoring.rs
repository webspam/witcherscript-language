use witcherscript_language::document::parse_document;
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{resolve_definition, SymbolDb, WorkspaceIndex};
use witcherscript_language::symbols::AccessLevel;

use crate::convert::wrap_method_snippet;

#[test]
fn rename_returns_edits_for_all_occurrences() {
    use witcherscript_language::resolve::find_references;

    let source = "function Make() {\n var x : int;\n x = 1;\n x = x + 1;\n}\n";
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &db,
        SourcePosition {
            line: 1,
            character: 5,
        },
    )
    .expect("local variable should resolve");

    let search_docs = vec![("file:///example.ws", &document)];
    let refs = find_references(&definition, &document, &search_docs, &db, true);

    assert!(
        refs.len() >= 4,
        "expected at least 4 occurrences (decl + 3 uses), got {}",
        refs.len()
    );
}

#[test]
fn rename_rejects_base_script_symbol() {
    use std::collections::HashSet;

    let base_source = "function BaseFunc() {}\n";
    let base_doc = parse_document(base_source).expect("should parse");
    let mut base_index = WorkspaceIndex::default();
    base_index.update_document("file:///base/base.ws", &base_doc);

    let caller_source = "function MyFunc() { BaseFunc(); }\n";
    let caller_doc = parse_document(caller_source).expect("should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///project/my.ws", &caller_doc);
    let db = SymbolDb::new(&workspace, &base_index);

    let definition = resolve_definition(
        "file:///project/my.ws",
        &caller_doc,
        &db,
        SourcePosition {
            line: 0,
            character: 20,
        },
    )
    .expect("BaseFunc call should resolve to base definition");

    assert_eq!(
        definition.uri, "file:///base/base.ws",
        "definition should point into the base scripts"
    );

    let base_uris: HashSet<String> = ["file:///base/base.ws".to_string()].into();
    assert!(
        base_uris.contains(&definition.uri),
        "rename should be rejected: symbol is declared in a base script"
    );
}

#[test]
fn rename_does_not_edit_base_scripts() {
    use std::collections::HashMap;
    use witcherscript_language::resolve::find_references;

    use crate::backend::rename_changes;

    // Base script declares CR4Player; one of its methods calls IsCiri()
    // unqualified (implicit `this`). Since 8023ddf the workspace @addMethod is
    // indexed as a real member of CR4Player, so this base call site resolves
    // into the workspace symbol and find_references reports it.
    let base_source = "class CR4Player {\n  function Foo() { IsCiri(); }\n}\n";
    let base_doc = parse_document(base_source).expect("base should parse");
    let base_doc_owned = parse_document(base_source).expect("base should parse");
    let mut base_index = WorkspaceIndex::default();
    base_index.update_document("file:///base/player.ws", &base_doc);
    let mut base_docs: HashMap<String, _> = HashMap::new();
    base_docs.insert("file:///base/player.ws".to_string(), base_doc_owned);

    // Workspace mod adds IsCiri() to CR4Player via @addMethod.
    let mod_source = "@addMethod(CR4Player)\nfunction IsCiri() {}\n";
    let mod_doc = parse_document(mod_source).expect("mod should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///mod/ciri.ws", &mod_doc);

    let db = SymbolDb::new(&workspace, &base_index);

    let definition = resolve_definition(
        "file:///mod/ciri.ws",
        &mod_doc,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    )
    .expect("@addMethod function name should resolve");
    assert!(
        !base_docs.contains_key(&definition.uri),
        "definition is in the workspace, so the existing guard lets the rename through"
    );

    let search_docs = vec![
        ("file:///base/player.ws", &base_doc),
        ("file:///mod/ciri.ws", &mod_doc),
    ];
    let refs = find_references(&definition, &mod_doc, &search_docs, &db, true);
    assert!(
        refs.iter().any(|(uri, _)| uri == "file:///base/player.ws"),
        "the base-script call site resolves into the @addMethod symbol"
    );

    let changes = rename_changes(&refs, "IsCiriRenamed", &base_docs);
    assert!(
        changes
            .keys()
            .all(|url| url.as_str() != "file:///base/player.ws"),
        "rename must not emit edits for read-only base-script files"
    );
    assert!(
        !changes.is_empty(),
        "rename should still emit edits for the workspace declaration"
    );
}

#[test]
fn wrap_method_snippet_plain_params() {
    let source = concat!(
        "class CPlayer {\n",
        "  public function CanParry(damage : int, attacker : CObject) : bool {}\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let method = db
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "CanParry")
        .expect("CanParry should be a member of CPlayer");

    let snippet = wrap_method_snippet(&method, &db);
    assert_eq!(
        snippet,
        "CanParry(damage : int, attacker : CObject) {\n\t$0\n\n\treturn wrappedMethod(damage, attacker);\n}"
    );
}

#[test]
fn wrap_method_snippet_optional_and_out_params() {
    let source = concat!(
        "class CPlayer {\n",
        "  public function Foo(a : int, optional b : float, out c : string) {}\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let method = db
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "Foo")
        .expect("Foo should be a member");

    let snippet = wrap_method_snippet(&method, &db);
    assert_eq!(
        snippet,
        "Foo(a : int, optional b : float, out c : string) {\n\twrappedMethod(a, b, c);\n\n\t$0\n}"
    );
}

#[test]
fn wrap_method_snippet_no_params() {
    let source = "class CPlayer {\n  public function OnSpawned() {}\n}\n";
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let method = db
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "OnSpawned")
        .expect("OnSpawned should be a member");

    let snippet = wrap_method_snippet(&method, &db);
    assert_eq!(snippet, "OnSpawned() {\n\twrappedMethod();\n\n\t$0\n}");
}

#[test]
fn wrap_method_snippet_event_uses_return_form() {
    // Events always use the return form so the caller can be reached after custom logic.
    let source = "class CPlayer {\n  public event OnDeath() {}\n}\n";
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let method = db
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "OnDeath")
        .expect("OnDeath should be a member");

    let snippet = wrap_method_snippet(&method, &db);
    assert_eq!(snippet, "OnDeath() {\n\t$0\n\n\treturn wrappedMethod();\n}");
}
