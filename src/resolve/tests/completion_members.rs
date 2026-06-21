use super::super::completion_members;
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

#[test]
fn definition_at_selection_disambiguates_member_by_container_when_stale() {
    let t = TestDb::new(concat!(
        "function Run() {}\n",
        "class CExample {\n",
        "  public function Run() {}\n",
        "}\n",
    ));
    let db = t.db();
    let def = db
        .definition_at_selection(t.primary_uri(), &(0..0), "Run", Some("CExample"))
        .expect("must resolve via container");

    assert_eq!(
        def.symbol.kind,
        SymbolKind::Method,
        "container must select the method, not the same-named top-level function"
    );
    assert_eq!(def.symbol.container_name.as_deref(), Some("CExample"));
}

#[test]
fn completion_after_dot_returns_public_members() {
    let t = TestDb::new(concat!(
        "class CPlayer {\n",
        "  public function GetHealth() : int {}\n",
        "  private var mHp : int;\n",
        "}\n",
        "function Test() {\n",
        "  var p : CPlayer;\n",
        "  p.$0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let members = completion_members(&uri, t.doc_for(&uri), &t.db(), pos);

    let names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    assert!(
        names.contains(&"GetHealth"),
        "public method should be in completions"
    );
    assert!(
        !names.contains(&"mHp"),
        "private field should not be in completions"
    );
}

#[test]
fn completes_partial_member_when_next_line_unterminated() {
    // Missing `;` makes tree-sitter glue `x.Ab` onto the next line's call.
    let t = TestDb::new(concat!(
        "class CPlayer {\n",
        "  public function AbortSign() {}\n",
        "}\n",
        "function Test() {\n",
        "  var x : CPlayer;\n",
        "  x.Ab$0\n",
        "  x.AbortSign();\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let members = completion_members(&uri, t.doc_for(&uri), &t.db(), pos);
    let names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    assert!(
        names.contains(&"AbortSign"),
        "partial member completion should offer AbortSign, got {names:?}"
    );
}

#[test]
fn completion_includes_inherited_members() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class A extends B {\n",
        "  public function Own() {}\n",
        "}\n",
        "function Test() {\n",
        "  var a : A;\n",
        "  a.$0\n",
        "}\n",
        "//- /b.ws\n",
        "class B {\n",
        "  public function Inherited() {}\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let members = completion_members(&uri, t.doc_for(&uri), &t.db(), pos);

    let names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    assert!(names.contains(&"Own"), "own method should appear");
    assert!(
        names.contains(&"Inherited"),
        "inherited method should appear"
    );
    let own_tier = members
        .iter()
        .find(|(_, d)| d.symbol.name == "Own")
        .map(|(t, _)| *t)
        .unwrap();
    let inherited_tier = members
        .iter()
        .find(|(_, d)| d.symbol.name == "Inherited")
        .map(|(t, _)| *t)
        .unwrap();
    assert!(
        own_tier < inherited_tier,
        "own members must have lower sort tier than inherited members"
    );
}
