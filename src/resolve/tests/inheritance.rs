use super::super::resolve_definition;
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;
use crate::symbols::{AccessLevel, SymbolKind};

#[test]
fn resolves_this_keyword_to_current_class() {
    let source = "class MyClass {\n function Test() {\n  this.Foo();\n }\n}\n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc);

    // cursor on 'this' (line 2, col 3)
    let definition = resolve_definition(
        "file:///a.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("this keyword should navigate to current class");

    assert_eq!(definition.symbol.name, "MyClass");
    assert_eq!(definition.symbol.kind, SymbolKind::Class);
}

#[test]
fn resolves_super_keyword_to_parent_class() {
    let source_a = "class A extends B {\n function Test() {\n  super.Method();\n }\n}\n";
    let source_b = "class B {\n function Method() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    // cursor on 'super' (line 2, col 3)
    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("super keyword should navigate to parent class");

    assert_eq!(definition.symbol.name, "B");
    assert_eq!(definition.symbol.kind, SymbolKind::Class);
}

#[test]
fn resolves_super_keyword_with_caret_at_end_of_word() {
    let source_a = "class A extends B {\n function Test() {\n  super.Method();\n }\n}\n";
    let source_b = "class B {\n function Method() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    // cursor just past the 'r' of "super" (line 2, col 7 — one past the end of the word)
    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 7,
        },
    )
    .expect("super keyword should resolve when caret is at end of word");

    assert_eq!(definition.symbol.name, "B");
    assert_eq!(definition.symbol.kind, SymbolKind::Class);
}

#[test]
fn resolves_inherited_method_via_workspace() {
    let source_a = "class A extends B {\n function Test() {\n  Inherited();\n }\n}\n";
    let source_b = "class B {\n function Inherited() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("inherited method should resolve");

    assert_eq!(definition.symbol.name, "Inherited");
    assert_eq!(definition.symbol.kind, SymbolKind::Method);
}

#[test]
fn class_without_explicit_extends_defaults_to_cobject() {
    let doc = make_doc("class A {}");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);
    // CObject is not in the index; find_member must terminate without looping.
    assert!(db
        .find_member("A", "someMethod", AccessLevel::Public)
        .is_none());
}

#[test]
fn baseless_class_inherits_cobject_members() {
    let source = concat!(
        "class CObject {\n",
        "  function GetCurrentStateName() : name {}\n",
        "}\n",
        "class PlayerAiming {}\n",
        "class CR4Player {\n",
        "  var playerAiming : PlayerAiming;\n",
        "  function Test() {\n",
        "    playerAiming.GetCurrentStateName();\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc);

    // cursor on 'GetCurrentStateName' (line 7, col 20)
    let definition = resolve_definition(
        "file:///a.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 7,
            character: 20,
        },
    )
    .expect("a class with no extends still inherits CObject's members");

    assert_eq!(definition.symbol.name, "GetCurrentStateName");
}

#[test]
fn engine_root_chain_does_not_loop_through_virtual_base() {
    let source = concat!(
        "class ISerializable {}\n",
        "class IScriptable extends ISerializable {}\n",
        "class CObject extends IScriptable {}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    // The root must not gain a virtual CObject base that closes the cycle.
    assert_eq!(db.superclass_of("ISerializable"), None);
    assert!(db
        .find_member("CObject", "missing", AccessLevel::Public)
        .is_none());
}

#[test]
fn resolves_inherited_method_unqualified_inside_subclass() {
    let source_a = "class A extends B {\n function Test() {\n  Inherited();\n }\n}\n";
    let source_b = "class B {\n function Inherited() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("unqualified inherited method should resolve inside subclass body");

    assert_eq!(definition.symbol.name, "Inherited");
}

#[test]
fn resolves_this_dot_inherited_method() {
    let source_a = "class A extends B {\n function Test() {\n  this.Inherited();\n }\n}\n";
    let source_b = "class B {\n function Inherited() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 8,
        },
    )
    .expect("this.Inherited() should resolve to superclass method");

    assert_eq!(definition.symbol.name, "Inherited");
}

#[test]
fn resolves_method_on_class_field_receiver() {
    let source = concat!(
        "class Foo {\n",
        "  private var gConfig : CInGameConfigWrapper;\n",
        "  function someFunc() {\n",
        "    gConfig.GetSpecialConfig();\n",
        "  }\n",
        "}\n",
        "class CInGameConfigWrapper {\n",
        "  function GetSpecialConfig() {}\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // cursor on 'GetSpecialConfig' (line 3, col 12)
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 3,
            character: 12,
        },
    )
    .expect("method on class field should resolve");

    assert_eq!(definition.symbol.name, "GetSpecialConfig");
}

#[test]
fn private_method_in_parent_resolves_from_subclass_for_navigation() {
    let source_a = "class A extends B {\n function Test() {\n  this.Secret();\n }\n}\n";
    let source_b = "class B {\n private function Secret() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 8,
        },
    )
    .expect("private parent method should still resolve so navigation works");

    assert_eq!(definition.symbol.name, "Secret");
    assert_eq!(definition.uri, "file:///b.ws");
}

#[test]
fn private_method_visible_within_own_class() {
    let source =
        "class A {\n private function Secret() {}\n function Test() {\n  this.Secret();\n }\n}\n";
    let doc = make_doc(source);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 3,
            character: 8,
        },
    )
    .expect("private method should be visible from within the same class");

    assert_eq!(definition.symbol.name, "Secret");
}

#[test]
fn protected_method_visible_in_subclass() {
    let source_a = "class A extends B {\n function Test() {\n  this.Guarded();\n }\n}\n";
    let source_b = "class B {\n protected function Guarded() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 8,
        },
    )
    .expect("protected method should be visible in a subclass");

    assert_eq!(definition.symbol.name, "Guarded");
}

#[test]
fn protected_method_not_visible_externally() {
    let source_a = "class A {\n function Test(b : B) {\n  b.Guarded();\n }\n}\n";
    let source_b = "class B {\n protected function Guarded() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 5,
        },
    );

    assert!(
        definition.is_none(),
        "protected method should not resolve from an unrelated external class"
    );
}

#[test]
fn unspecified_access_defaults_to_public() {
    let source_a = "class A {\n function Test(b : B) {\n  b.Open();\n }\n}\n";
    let source_b = "class B {\n function Open() {}\n}\n";
    let doc_a = make_doc(source_a);
    let doc_b = make_doc(source_b);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 5,
        },
    )
    .expect("method with no specifier should default to public and be visible externally");

    assert_eq!(definition.symbol.name, "Open");
}

#[test]
fn state_parent_dot_resolves_to_owner_class_method() {
    // parent.X inside a state should resolve to X on the owning class (public only).
    let source = concat!(
        "class CPlayer {\n",
        "  function GetHealth() : int {}\n",
        "}\n",
        "state Idle in CPlayer {\n",
        "  function Test() {\n",
        "    parent.GetHealth();\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // cursor on 'GetHealth' (line 5, col 11)
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 11,
        },
    )
    .expect("parent.Method() in a state should resolve to the owner class method");

    assert_eq!(definition.symbol.name, "GetHealth");
}

#[test]
fn state_parent_dot_cannot_see_protected_owner_method() {
    // parent confers no inheritance relationship; protected members of the owner
    // are not accessible via parent.
    let source = concat!(
        "class CPlayer {\n",
        "  protected function InternalTick() {}\n",
        "}\n",
        "state Idle in CPlayer {\n",
        "  function Test() {\n",
        "    parent.InternalTick();\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // cursor on 'InternalTick' (line 5, col 11)
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 11,
        },
    );

    assert!(
        definition.is_none(),
        "parent.X in a state must not resolve protected members of the owner class"
    );
}

#[test]
fn state_without_explicit_extends_defaults_to_cscriptablestate() {
    let source = concat!(
        "statemachine class Owner {}\n",
        "state SpawnBoatLatentHack in Owner {}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    assert_eq!(
        db.superclass_of("SpawnBoatLatentHack").as_deref(),
        Some("CScriptableState")
    );
    // CScriptableState is not declared; the chain must terminate without looping.
    assert!(db
        .find_member("SpawnBoatLatentHack", "someMethod", AccessLevel::Public)
        .is_none());
}

#[test]
fn baseless_state_inherits_cscriptablestate_members() {
    let source = concat!(
        "class IScriptable {}\n",
        "class CScriptableState extends IScriptable {\n",
        "  function OnEnterState(prevName : name) {}\n",
        "}\n",
        "statemachine class Owner {}\n",
        "state SpawnBoatLatentHack in Owner {\n",
        "  entry function Run() {\n",
        "    OnEnterState('Foo');\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // cursor on 'OnEnterState' inside Run (line 7, col 5)
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 7,
            character: 5,
        },
    )
    .expect("a state with no extends still inherits CScriptableState's members");

    assert_eq!(definition.symbol.name, "OnEnterState");
    assert_eq!(
        definition.symbol.container_name.as_deref(),
        Some("CScriptableState")
    );
}

#[test]
fn super_dot_x_resolves_in_baseless_type() {
    struct Case {
        label: &'static str,
        source: &'static str,
        cursor: SourcePosition,
        expected_member: &'static str,
        expected_container: &'static str,
    }
    let cases = [
        Case {
            label: "baseless class falls back to CObject",
            source: concat!(
                "class CObject {\n",
                "  function GetName() : name {}\n",
                "}\n",
                "class Foo {\n",
                "  function Bar() {\n",
                "    super.GetName();\n",
                "  }\n",
                "}\n",
            ),
            cursor: SourcePosition {
                line: 5,
                character: 12,
            },
            expected_member: "GetName",
            expected_container: "CObject",
        },
        Case {
            label: "baseless state falls back to CScriptableState",
            source: concat!(
                "class IScriptable {}\n",
                "class CScriptableState extends IScriptable {\n",
                "  function OnEnterState(prevName : name) {}\n",
                "}\n",
                "statemachine class Owner {}\n",
                "state SpawnBoatLatentHack in Owner {\n",
                "  entry function Run() {\n",
                "    super.OnEnterState('Foo');\n",
                "  }\n",
                "}\n",
            ),
            cursor: SourcePosition {
                line: 7,
                character: 12,
            },
            expected_member: "OnEnterState",
            expected_container: "CScriptableState",
        },
    ];
    for c in cases {
        let doc = make_doc(c.source);
        let mut index = WorkspaceIndex::default();
        index.update_document("file:///test.ws", &doc);

        let definition = resolve_definition(
            "file:///test.ws",
            &doc,
            &SymbolDb::new(&index, &WorkspaceIndex::default()),
            c.cursor,
        )
        .unwrap_or_else(|| panic!("case {}: super.X did not resolve", c.label));
        assert_eq!(
            definition.symbol.name, c.expected_member,
            "case {}: member name mismatch",
            c.label
        );
        assert_eq!(
            definition.symbol.container_name.as_deref(),
            Some(c.expected_container),
            "case {}: container mismatch",
            c.label
        );
    }
}

#[test]
fn state_method_resolves_through_extends_chain() {
    let source = concat!(
        "statemachine class Owner {}\n",
        "state Base in Owner { function Help() {} }\n",
        "state Mid in Owner extends Base {}\n",
        "state Leaf in Owner extends Mid {\n",
        "  entry function Run() {\n",
        "    Help();\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // cursor on 'Help' inside Leaf.Run (line 5, col 4)
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 4,
        },
    )
    .expect("unqualified call to a method inherited via state extends should resolve");

    assert_eq!(definition.symbol.name, "Help");
    assert_eq!(definition.symbol.container_name.as_deref(), Some("Base"));
}
