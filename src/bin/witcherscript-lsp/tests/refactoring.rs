use rstest::rstest;
use std::collections::HashMap;

use witcherscript_language::document::parse_document;
use witcherscript_language::formatter::ColonSpacing;
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{
    SymbolDb, WorkspaceIndex, find_references, resolve_definition,
};
use witcherscript_language::symbols::AccessLevel;
use witcherscript_language::test_support::TestDb;

use crate::convert::{replace_method_snippet, wrap_method_snippet};

#[test]
fn renaming_a_state_rewrites_class_name_occurrences_with_the_new_class_name() {
    use crate::references_rename::rename_changes;

    let t = TestDb::new(
        "statemachine class C {}\nstate $0Sleep in C {}\nfunction F(s : CStateSleep) {}\n",
    );
    let (uri, pos) = t.cursor();
    let db = t.db();
    let def = resolve_definition(&uri, t.doc_for(&uri), &db, pos).expect("state resolves");
    let refs = find_references(&def, t.doc_for(&uri), &t.search_docs(), &db, true);
    let new_type_name = def.symbol.renamed_type_name("Nap");

    let changes = rename_changes(&refs, "Nap", &new_type_name, &HashMap::new());
    let mut edits: Vec<_> = changes.into_values().flatten().collect();
    edits.sort_by_key(|edit| (edit.range.start.line, edit.range.start.character));
    let new_texts: Vec<&str> = edits.iter().map(|edit| edit.new_text.as_str()).collect();

    assert_eq!(
        new_texts,
        ["Nap", "CStateNap"],
        "the short-name declaration becomes `Nap`, the class-name usage becomes `CStateNap`"
    );
}

#[test]
fn rename_returns_edits_for_all_occurrences() {
    let t = TestDb::new("function Make() {\n var $0x : int;\n x = 1;\n x = x + 1;\n}\n");
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let def = resolve_definition(&uri, doc, &t.db(), pos).expect("local var should resolve");
    let refs = find_references(&def, doc, &t.search_docs(), &t.db(), true);
    assert!(
        refs.len() >= 4,
        "expected at least 4 occurrences (decl + 3 uses), got {}",
        refs.len()
    );
}

#[test]
fn rename_definition_in_base_script_is_flagged() {
    let base_doc = parse_document("function BaseFunc() {}\n").expect("parse");
    let mut base_index = WorkspaceIndex::default();
    base_index.update_document("file:///base/base.ws", &base_doc);

    let caller_doc = parse_document("function MyFunc() { BaseFunc(); }\n").expect("parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///project/my.ws", &caller_doc);
    let db = SymbolDb::new(&workspace, &base_index);

    let def = resolve_definition(
        "file:///project/my.ws",
        &caller_doc,
        &db,
        SourcePosition {
            line: 0,
            character: 20,
        },
    )
    .expect("BaseFunc call should resolve into the base scripts");

    assert_eq!(def.uri, "file:///base/base.ws");
}

#[test]
fn rename_does_not_edit_base_scripts() {
    use crate::references_rename::rename_changes;

    let base_doc = parse_document("class CR4Player {\n  function Foo() { IsCiri(); }\n}\n")
        .expect("base should parse");
    let base_doc_owned = parse_document("class CR4Player {\n  function Foo() { IsCiri(); }\n}\n")
        .expect("base should parse");
    let mut base_index = WorkspaceIndex::default();
    base_index.update_document("file:///base/player.ws", &base_doc);
    let mut base_docs: HashMap<String, std::sync::Arc<_>> = HashMap::new();
    base_docs.insert(
        "file:///base/player.ws".to_string(),
        std::sync::Arc::new(base_doc_owned),
    );

    let mod_doc =
        parse_document("@addMethod(CR4Player)\nfunction IsCiri() {}\n").expect("mod should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///mod/ciri.ws", &mod_doc);
    let db = SymbolDb::new(&workspace, &base_index);

    let def = resolve_definition(
        "file:///mod/ciri.ws",
        &mod_doc,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    )
    .expect("@addMethod function name should resolve");
    assert!(!base_docs.contains_key(&def.uri));

    let search_docs = vec![
        ("file:///base/player.ws", &base_doc),
        ("file:///mod/ciri.ws", &mod_doc),
    ];
    let refs = find_references(&def, &mod_doc, &search_docs, &db, true);
    assert!(
        refs.iter().any(|r| r.uri == "file:///base/player.ws"),
        "the base-script call site is a reference"
    );

    let changes = rename_changes(&refs, "IsCiriRenamed", "IsCiriRenamed", &base_docs);
    assert!(
        changes
            .keys()
            .all(|url| url.as_str() != "file:///base/player.ws")
    );
    assert!(!changes.is_empty());
}

#[rstest]
#[case::plain_params(
    "class CPlayer {\n  public function CanParry(damage : int, attacker : CObject) : bool {}\n}\n",
    "CanParry",
    "CanParry(damage : int, attacker : CObject) {\n\t$0\n\n\treturn wrappedMethod(damage, attacker);\n}"
)]
#[case::optional_and_out_params(
    "class CPlayer {\n  public function Foo(a : int, optional b : float, out c : string) {}\n}\n",
    "Foo",
    "Foo(a : int, optional b : float, out c : string) {\n\twrappedMethod(a, b, c);\n\n\t$0\n}"
)]
#[case::no_params(
    "class CPlayer {\n  public function OnSpawned() {}\n}\n",
    "OnSpawned",
    "OnSpawned() {\n\twrappedMethod();\n\n\t$0\n}"
)]
#[case::event_uses_return_form(
    "class CPlayer {\n  public event OnDeath() {}\n}\n",
    "OnDeath",
    "OnDeath() {\n\t$0\n\n\treturn wrappedMethod();\n}"
)]
fn wrap_method_snippet_shapes(
    #[case] source: &str,
    #[case] method_name: &str,
    #[case] expected: &str,
) {
    let t = TestDb::new(source);
    let method = t
        .db()
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == method_name)
        .unwrap_or_else(|| panic!("{method_name} should be a member of CPlayer"));
    let snippet = wrap_method_snippet(&method, &t.db(), ColonSpacing::Spaced);
    assert_eq!(snippet, expected);
}

#[test]
fn replace_method_snippet_omits_wrapped_method() {
    let t = TestDb::new(
        "class CPlayer {\n  public function CanParry(damage : int, attacker : CObject) : bool {}\n}\n",
    );
    let method = t
        .db()
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "CanParry")
        .expect("CanParry should be a member of CPlayer");
    let snippet = replace_method_snippet(&method, &t.db(), ColonSpacing::Spaced);
    assert_eq!(
        snippet,
        "CanParry(damage : int, attacker : CObject) {\n\t$0\n}"
    );
}
