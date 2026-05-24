use super::super::{completion_members, resolve_definition, statement_completions};
use super::{index_docs, make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

#[test]
fn add_method_body_sees_class_members() {
    let base = make_doc(concat!(
        "class CPlayer {\n",
        "  private var mHp : int;\n",
        "  public function Heal() {}\n",
        "}\n",
    ));
    let modd = make_doc("@addMethod(CPlayer)\nfunction Boost() {\n  \n}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = statement_completions(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    let members: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(result.has_this, "has_this must be true inside @addMethod");
    assert!(
        members.contains(&"mHp"),
        "private field of target class must be offered"
    );
    assert!(
        members.contains(&"Heal"),
        "method of target class must be offered"
    );
}

#[test]
fn wrap_method_body_sees_members_and_super() {
    let base = make_doc(concat!(
        "class CBase {\n",
        "  public function BaseMethod() {}\n",
        "}\n",
        "class CPlayer extends CBase {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
    ));
    let modd = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {\n  \n}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = statement_completions(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    let members: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        result.has_super,
        "has_super must be true — target extends CBase"
    );
    assert!(
        members.contains(&"OnSpawned"),
        "own member of target class must be offered"
    );
    assert!(
        members.contains(&"BaseMethod"),
        "inherited member of target class must be offered"
    );
}

#[test]
fn replace_method_body_behaves_like_wrap() {
    let base = make_doc(concat!(
        "class CBase {\n",
        "  public function BaseMethod() {}\n",
        "}\n",
        "class CPlayer extends CBase {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
    ));
    let modd = make_doc("@replaceMethod(CPlayer)\nfunction OnSpawned() {\n  \n}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = statement_completions(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    let members: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        result.has_super,
        "@replaceMethod must expose super like @wrapMethod"
    );
    assert!(
        members.contains(&"BaseMethod"),
        "inherited member must be offered"
    );
}

#[test]
fn annotated_body_sees_sibling_add_method() {
    let base = make_doc("class CPlayer {\n  public function Heal() {}\n}\n");
    let mod_a = make_doc("@addMethod(CPlayer)\nfunction Boost() {}\n");
    let mod_b = make_doc("@wrapMethod(CPlayer)\nfunction Heal() {\n  \n}\n");
    let index = index_docs(&[
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///b.ws", &mod_b),
    ]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = statement_completions(
        "file:///b.ws",
        &mod_b,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    let members: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        members.contains(&"Boost"),
        "an @addMethod sibling must be visible inside another annotated body"
    );
}

#[test]
fn add_method_body_this_resolves() {
    let base = make_doc("class CPlayer {\n  public function Heal() {}\n}\n");
    let modd = make_doc("@addMethod(CPlayer)\nfunction Boost() {\n  this.Heal();\n}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let definition = resolve_definition(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    )
    .expect("`this` inside @addMethod must resolve to the target class");
    assert_eq!(definition.symbol.name, "CPlayer");
    assert_eq!(definition.symbol.kind, SymbolKind::Class);
}

#[test]
fn wrap_method_body_super_member_resolves() {
    let base = make_doc(concat!(
        "class CBase {\n",
        "  public function BaseMethod() {}\n",
        "}\n",
        "class CPlayer extends CBase {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
    ));
    let modd = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {\n  super.BaseMethod();\n}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let definition = resolve_definition(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 2,
            character: 9,
        },
    )
    .expect("super member inside @wrapMethod must resolve to the base class method");
    assert_eq!(definition.symbol.name, "BaseMethod");
    assert_eq!(definition.symbol.kind, SymbolKind::Method);
}

#[test]
fn add_method_on_state_offers_parent_members() {
    let base = make_doc(concat!(
        "statemachine class CMachine {\n",
        "  public function MachineMethod() {}\n",
        "}\n",
        "state SomeState in CMachine {\n",
        "}\n",
    ));
    let modd = make_doc("@addMethod(SomeState)\nfunction Extra() {\n  parent.\n}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let members = completion_members(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 2,
            character: 9,
        },
    );
    let names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    assert!(
        names.contains(&"MachineMethod"),
        "`parent.` inside @addMethod on a state must offer owner-class members"
    );
}

#[test]
fn annotated_function_own_locals_and_params_still_work() {
    let base = make_doc("class CPlayer {\n  public function Heal() {}\n}\n");
    let modd = make_doc(concat!(
        "@addMethod(CPlayer)\n",
        "function Boost(amount : int) {\n",
        "  var scale : int;\n",
        "  scale;\n",
        "}\n",
    ));
    let index = index_docs(&[("file:///base.ws", &base), ("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = statement_completions(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 3,
            character: 2,
        },
    );
    let locals: Vec<&str> = result
        .locals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        locals.contains(&"amount"),
        "own parameter must still appear"
    );
    assert!(locals.contains(&"scale"), "own local must still appear");
}

#[test]
fn wrapped_method_locals_not_in_scope() {
    let base = make_doc(concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {\n",
        "    var secret : int;\n",
        "  }\n",
        "}\n",
    ));
    let modd = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {\n  \n}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = statement_completions(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    let visible: Vec<&str> = result
        .locals
        .iter()
        .chain(result.members.iter())
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        !visible.contains(&"secret"),
        "the wrapped method's locals must not be in scope"
    );
}

#[test]
fn add_method_unknown_class_no_panic() {
    let modd = make_doc("@addMethod(CDoesNotExist)\nfunction Boost() {\n  \n}\n");
    let index = index_docs(&[("file:///mod.ws", &modd)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    // Must not panic even though the target class does not exist.
    let result = statement_completions(
        "file:///mod.ws",
        &modd,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    assert!(
        result.has_this,
        "has_this is still true — the context name is set"
    );
    assert!(!result.has_super, "unknown class has no known base");
}
