use super::super::completion_members;
use crate::test_support::TestDb;

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
