use super::super::{completion_members, resolve_definition, statement_completions};
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

#[test]
fn add_method_body_sees_class_members() {
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CPlayer {\n",
        "  private var mHp : int;\n",
        "  public function Heal() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "@addMethod(CPlayer)\n",
        "function Boost() {\n",
        "  $0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
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
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CBase {\n",
        "  public function BaseMethod() {}\n",
        "}\n",
        "class CPlayer extends CBase {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "@wrapMethod(CPlayer)\n",
        "function OnSpawned() {\n",
        "  $0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
    let members: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        result.has_super,
        "has_super must be true - target extends CBase"
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
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CBase {\n",
        "  public function BaseMethod() {}\n",
        "}\n",
        "class CPlayer extends CBase {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "@replaceMethod(CPlayer)\n",
        "function OnSpawned() {\n",
        "  $0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
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
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CPlayer {\n",
        "  public function Heal() {}\n",
        "}\n",
        "//- /a.ws\n",
        "@addMethod(CPlayer)\n",
        "function Boost() {}\n",
        "//- /b.ws\n",
        "@wrapMethod(CPlayer)\n",
        "function Heal() {\n",
        "  $0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
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
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CPlayer {\n",
        "  public function Heal() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "@addMethod(CPlayer)\n",
        "function Boost() {\n",
        "  thi$0s.Heal();\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let definition = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("`this` inside @addMethod must resolve to the target class");
    assert_eq!(definition.symbol.name, "CPlayer");
    assert_eq!(definition.symbol.kind, SymbolKind::Class);
}

#[test]
fn wrap_method_body_super_member_resolves() {
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CBase {\n",
        "  public function BaseMethod() {}\n",
        "}\n",
        "class CPlayer extends CBase {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "@wrapMethod(CPlayer)\n",
        "function OnSpawned() {\n",
        "  super.B$0aseMethod();\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let definition = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("super member inside @wrapMethod must resolve to the base class method");
    assert_eq!(definition.symbol.name, "BaseMethod");
    assert_eq!(definition.symbol.kind, SymbolKind::Method);
}

#[test]
fn add_method_on_state_offers_parent_members() {
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "statemachine class CMachine {\n",
        "  public function MachineMethod() {}\n",
        "}\n",
        "state SomeState in CMachine {\n",
        "}\n",
        "//- /mod.ws\n",
        "@addMethod(SomeState)\n",
        "function Extra() {\n",
        "  parent.$0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let members = completion_members(&uri, t.doc_for(&uri), &t.db(), pos);
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
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CPlayer {\n",
        "  public function Heal() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "@addMethod(CPlayer)\n",
        "function Boost(amount : int) {\n",
        "  var scale : int;\n",
        "  $0scale;\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
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
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CPlayer {\n",
        "  public function OnSpawned() {\n",
        "    var secret : int;\n",
        "  }\n",
        "}\n",
        "//- /mod.ws\n",
        "@wrapMethod(CPlayer)\n",
        "function OnSpawned() {\n",
        "  $0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
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
    let t = TestDb::new(concat!(
        "@addMethod(CDoesNotExist)\n",
        "function Boost() {\n",
        "  $0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert!(
        result.has_this,
        "has_this is still true - the context name is set"
    );
    assert!(!result.has_super, "unknown class has no known base");
}
