use rstest::rstest;
use std::collections::HashMap;

use lsp_types::{PrepareRenameResponse, TextEdit};
use witcherscript_language::document::{ParsedDocument, parse_document};
use witcherscript_language::formatter::ColonSpacing;
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{
    SymbolDb, WorkspaceIndex, find_references, resolve_definition,
};
use witcherscript_language::symbols::AccessLevel;
use witcherscript_language::test_support::TestDb;

use crate::convert::{replace_method_snippet, wrap_method_snippet};

fn rename_edits(t: &TestDb, new_name: &str) -> Vec<TextEdit> {
    use crate::references_rename::rename_changes;

    let (uri, pos) = t.cursor();
    let db = t.db();
    let doc = t.doc_for(&uri);
    let def = resolve_definition(&uri, doc, &db, pos).expect("rename target resolves");
    let refs = find_references(&def, doc, &t.search_docs(), &db, true);
    let changes = rename_changes(&refs, def.symbol.short_name_from(new_name), &HashMap::new());
    changes.into_values().flatten().collect()
}

fn edit_byte_range(doc: &ParsedDocument, edit: &TextEdit) -> std::ops::Range<usize> {
    let to_byte = |position: lsp_types::Position| {
        doc.line_index
            .position_to_byte(
                &doc.source,
                SourcePosition {
                    line: position.line,
                    character: position.character,
                },
            )
            .expect("edit position lies inside the document")
    };
    to_byte(edit.range.start)..to_byte(edit.range.end)
}

fn renamed_source(t: &TestDb, new_name: &str) -> String {
    let doc = t.doc_for(t.primary_uri());
    let mut edits = rename_edits(t, new_name);
    edits.sort_by_key(|edit| std::cmp::Reverse(edit_byte_range(doc, edit).start));
    let mut source = doc.source.clone();
    for edit in &edits {
        source.replace_range(edit_byte_range(doc, edit), &edit.new_text);
    }
    source
}

const RENAMED_STATE_SOURCE: &str =
    "statemachine class C {}\nstate Nap in C {}\nfunction F(s : CStateNap) {}\n";

#[rstest]
#[case::from_declaration(
    "statemachine class C {}\nstate $0Sleep in C {}\nfunction F(s : CStateSleep) {}\n",
    "Nap"
)]
#[case::from_class_name_with_short_name(
    "statemachine class C {}\nstate Sleep in C {}\nfunction F(s : $0CStateSleep) {}\n",
    "Nap"
)]
#[case::from_class_name_with_class_name(
    "statemachine class C {}\nstate Sleep in C {}\nfunction F(s : $0CStateSleep) {}\n",
    "CStateNap"
)]
fn renaming_a_state_rewrites_only_its_short_name(#[case] source: &str, #[case] new_name: &str) {
    let t = TestDb::new(source);
    assert_eq!(
        renamed_source(&t, new_name),
        RENAMED_STATE_SOURCE,
        "the declaration and the class-name usage must both end up spelling `Nap`"
    );
}

#[test]
fn renaming_a_state_never_edits_the_owner_prefix() {
    let t = TestDb::new(
        "statemachine class C {}\nstate Sleep in C {}\nfunction F(s : $0CStateSleep) {}\n",
    );
    let doc = t.doc_for(t.primary_uri());
    let replaced: Vec<&str> = rename_edits(&t, "Nap")
        .iter()
        .map(|edit| &doc.source[edit_byte_range(doc, edit)])
        .collect();
    assert_eq!(
        replaced,
        ["Sleep", "Sleep"],
        "every edit must replace only the short name, never the `CState` prefix"
    );
}

#[rstest]
#[case::class_name_usage(
    "statemachine class C {}\nstate Sleep in C {}\nfunction F(s : $0CStateSleep) {}\n",
    Some("Sleep")
)]
#[case::short_name_declaration(
    "statemachine class C {}\nstate $0Sleep in C {}\nfunction F(s : CStateSleep) {}\n",
    None
)]
#[case::plain_class("class $0CPlain {}\nfunction F(p : CPlain) {}\n", None)]
fn prepare_rename_offers_the_short_name_on_state_class_names(
    #[case] source: &str,
    #[case] expected_placeholder: Option<&str>,
) {
    use crate::references_rename::prepare_rename_response;

    let t = TestDb::new(source);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let def = resolve_definition(&uri, doc, &t.db(), pos).expect("rename target resolves");
    let placeholder = match prepare_rename_response(&def, doc, pos) {
        PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. } => Some(placeholder),
        PrepareRenameResponse::DefaultBehavior { .. } => None,
        PrepareRenameResponse::Range(_) => panic!("prepare rename must not return a bare range"),
    };
    assert_eq!(
        placeholder.as_deref(),
        expected_placeholder,
        "only a state's class-name spelling swaps the placeholder for the short name"
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

    let changes = rename_changes(&refs, "IsCiriRenamed", &base_docs);
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
