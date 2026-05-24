use rstest::rstest;

use super::super::resolve_definition;
use crate::symbols::{AccessLevel, SymbolKind};
use crate::test_support::TestDb;

#[rstest]
#[case::this_keyword_self_class(
    "class MyClass {\n function Test() {\n  th$0is.Foo();\n }\n}\n",
    "MyClass",
    SymbolKind::Class
)]
#[case::super_keyword_at_start(
    "//- /a.ws\n\
     class A extends B {\n function Test() {\n  $0super.Method();\n }\n}\n\
     //- /b.ws\n\
     class B {\n function Method() {}\n}\n",
    "B",
    SymbolKind::Class
)]
#[case::super_keyword_at_word_end(
    "//- /a.ws\n\
     class A extends B {\n function Test() {\n  super$0.Method();\n }\n}\n\
     //- /b.ws\n\
     class B {\n function Method() {}\n}\n",
    "B",
    SymbolKind::Class
)]
fn keyword_resolves_to_class(
    #[case] fixture: &str,
    #[case] expected_name: &str,
    #[case] expected_kind: SymbolKind,
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let def =
        resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("keyword should resolve");
    assert_eq!(def.symbol.name, expected_name);
    assert_eq!(def.symbol.kind, expected_kind);
}

#[rstest]
#[case::inherited_method_via_workspace(
    "//- /a.ws\n\
     class A extends B {\n function Test() {\n  $0Inherited();\n }\n}\n\
     //- /b.ws\n\
     class B {\n function Inherited() {}\n}\n",
    "Inherited",
    None,
    None
)]
#[case::baseless_class_inherits_cobject(
    "class CObject {\n  function GetCurrentStateName() : name {}\n}\n\
     class PlayerAiming {}\n\
     class CR4Player {\n  var playerAiming : PlayerAiming;\n  function Test() {\n    playerAiming.$0GetCurrentStateName();\n  }\n}\n",
    "GetCurrentStateName",
    None,
    None,
)]
#[case::unqualified_inherited_in_subclass(
    "//- /a.ws\n\
     class A extends B {\n function Test() {\n  $0Inherited();\n }\n}\n\
     //- /b.ws\n\
     class B {\n function Inherited() {}\n}\n",
    "Inherited",
    None,
    None
)]
#[case::this_dot_inherited(
    "//- /a.ws\n\
     class A extends B {\n function Test() {\n  this.$0Inherited();\n }\n}\n\
     //- /b.ws\n\
     class B {\n function Inherited() {}\n}\n",
    "Inherited",
    None,
    None
)]
#[case::method_on_class_field_receiver(
    "class Foo {\n  private var gConfig : CInGameConfigWrapper;\n  function someFunc() {\n    gConfig.$0GetSpecialConfig();\n  }\n}\n\
     class CInGameConfigWrapper {\n  function GetSpecialConfig() {}\n}\n",
    "GetSpecialConfig",
    None,
    None,
)]
#[case::private_in_parent_navigable_from_subclass(
    "//- /a.ws\n\
     class A extends B {\n function Test() {\n  this.$0Secret();\n }\n}\n\
     //- /b.ws\n\
     class B {\n private function Secret() {}\n}\n",
    "Secret",
    None,
    Some("file:///b.ws")
)]
#[case::private_visible_within_own_class(
    "class A {\n private function Secret() {}\n function Test() {\n  this.$0Secret();\n }\n}\n",
    "Secret",
    None,
    None
)]
#[case::protected_visible_in_subclass(
    "//- /a.ws\n\
     class A extends B {\n function Test() {\n  this.$0Guarded();\n }\n}\n\
     //- /b.ws\n\
     class B {\n protected function Guarded() {}\n}\n",
    "Guarded",
    None,
    None
)]
#[case::unspecified_access_is_public(
    "//- /a.ws\n\
     class A {\n function Test(b : B) {\n  b.$0Open();\n }\n}\n\
     //- /b.ws\n\
     class B {\n function Open() {}\n}\n",
    "Open",
    None,
    None
)]
#[case::state_parent_dot_to_owner_method(
    "class CPlayer {\n  function GetHealth() : int {}\n}\n\
     state Idle in CPlayer {\n  function Test() {\n    parent.$0GetHealth();\n  }\n}\n",
    "GetHealth",
    None,
    None
)]
#[case::baseless_state_inherits_cscriptablestate(
    "class IScriptable {}\n\
     class CScriptableState extends IScriptable {\n  function OnEnterState(prevName : name) {}\n}\n\
     statemachine class Owner {}\n\
     state SpawnBoatLatentHack in Owner {\n  entry function Run() {\n    $0OnEnterState('Foo');\n  }\n}\n",
    "OnEnterState",
    Some("CScriptableState"),
    None,
)]
#[case::super_dot_baseless_class_falls_back_to_cobject(
    "class CObject {\n  function GetName() : name {}\n}\n\
     class Foo {\n  function Bar() {\n    super.$0GetName();\n  }\n}\n",
    "GetName",
    Some("CObject"),
    None
)]
#[case::super_dot_baseless_state_falls_back_to_cscriptablestate(
    "class IScriptable {}\n\
     class CScriptableState extends IScriptable {\n  function OnEnterState(prevName : name) {}\n}\n\
     statemachine class Owner {}\n\
     state SpawnBoatLatentHack in Owner {\n  entry function Run() {\n    super.$0OnEnterState('Foo');\n  }\n}\n",
    "OnEnterState",
    Some("CScriptableState"),
    None,
)]
#[case::state_method_through_extends_chain(
    "statemachine class Owner {}\n\
     state Base in Owner { function Help() {} }\n\
     state Mid in Owner extends Base {}\n\
     state Leaf in Owner extends Mid {\n  entry function Run() {\n    $0Help();\n  }\n}\n",
    "Help",
    Some("Base"),
    None
)]
fn method_resolution_at_cursor(
    #[case] fixture: &str,
    #[case] expected_name: &str,
    #[case] expected_container: Option<&str>,
    #[case] expected_uri: Option<&str>,
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let def =
        resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("method should resolve");
    assert_eq!(def.symbol.name, expected_name);
    if let Some(c) = expected_container {
        assert_eq!(def.symbol.container_name.as_deref(), Some(c));
    }
    if let Some(u) = expected_uri {
        assert_eq!(def.uri, u);
    }
}

#[rstest]
#[case::protected_method_blocked_externally(
    "//- /a.ws\n\
     class A {\n function Test(b : B) {\n  b.$0Guarded();\n }\n}\n\
     //- /b.ws\n\
     class B {\n protected function Guarded() {}\n}\n"
)]
#[case::state_parent_dot_blocked_for_protected(
    "class CPlayer {\n  protected function InternalTick() {}\n}\n\
     state Idle in CPlayer {\n  function Test() {\n    parent.$0InternalTick();\n  }\n}\n"
)]
fn method_resolution_blocked(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos);
    assert!(def.is_none(), "expected no resolution");
}

#[test]
fn class_without_explicit_extends_defaults_to_cobject() {
    let t = TestDb::new("class A {}");
    assert!(t
        .db()
        .find_member("A", "someMethod", AccessLevel::Public)
        .is_none());
}

#[test]
fn engine_root_chain_does_not_loop_through_virtual_base() {
    let t = TestDb::new(concat!(
        "class ISerializable {}\n",
        "class IScriptable extends ISerializable {}\n",
        "class CObject extends IScriptable {}\n",
    ));
    assert_eq!(t.db().superclass_of("ISerializable"), None);
    assert!(t
        .db()
        .find_member("CObject", "missing", AccessLevel::Public)
        .is_none());
}

#[test]
fn state_without_explicit_extends_defaults_to_cscriptablestate() {
    let t = TestDb::new(concat!(
        "statemachine class Owner {}\n",
        "state SpawnBoatLatentHack in Owner {}\n",
    ));
    assert_eq!(
        t.db().superclass_of("SpawnBoatLatentHack").as_deref(),
        Some("CScriptableState")
    );
    assert!(t
        .db()
        .find_member("SpawnBoatLatentHack", "someMethod", AccessLevel::Public)
        .is_none());
}
