use super::super::{
    after_wrap_method_completions, annotation_arg_completions, annotation_name_completions,
    completion_members, resolve_definition, statement_completions, AfterWrapMethodCompletions,
};
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

#[test]
fn annotation_arg_completions_offers_classes_for_all_modding_annotations() {
    for (annotation, should_fire) in [
        ("@addField", true),
        ("@addMethod", true),
        ("@wrapMethod", true),
        ("@replaceMethod", true),
        ("@someUnknownAnnotation", false),
    ] {
        // Closed parens so tree-sitter emits a well-formed annotation node.
        let source = format!(
            "class CPlayer {{}}\n\
             struct SData {{}}\n\
             enum EDir {{ North = 0 }}\n\
             {annotation}(CPlayer)\n"
        );
        let doc = make_doc(&source);
        let mut index = WorkspaceIndex::default();
        index.update_document("file:///test.ws", &doc);

        // Cursor on the 'C' of 'CPlayer': past the annotation name and '('.
        let character = annotation.len() as u32 + 1;
        let completions = annotation_arg_completions(
            &doc,
            &SymbolDb::new(&index, &WorkspaceIndex::default()),
            SourcePosition { line: 3, character },
        );

        let names: Vec<&str> = completions.iter().map(|d| d.symbol.name.as_str()).collect();
        if should_fire {
            assert!(
                names.contains(&"CPlayer"),
                "{annotation}: class should be offered inside parens"
            );
            assert!(
                !names.contains(&"SData"),
                "{annotation}: struct should not be offered inside parens"
            );
            assert!(
                !names.contains(&"EDir"),
                "{annotation}: enum should not be offered inside parens"
            );
        } else {
            assert!(
                completions.is_empty(),
                "{annotation}: unknown annotation must not get class completion"
            );
        }
    }
}

#[test]
fn annotation_arg_completions_empty_outside_annotation() {
    let source = concat!("class CPlayer {}\n", "function Test() {}\n",);
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let completions = annotation_arg_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 9,
        },
    );

    assert!(
        completions.is_empty(),
        "annotation_arg_completions must not fire outside an annotation"
    );
}

#[test]
fn annotation_arg_completions_empty_after_closing_paren() {
    // Cursor is after the ')' — should not offer anything.
    let source = "@wrapMethod(CPlayer) \n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let completions = annotation_arg_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 21,
        },
    );

    assert!(
        completions.is_empty(),
        "annotation_arg_completions must not fire after the closing paren"
    );
}

// --- after_wrap_method_completions ---
//
// Stage 1: cursor directly after @wrapMethod(CClass) → FunctionKeyword
// Stage 2: cursor after `function` that follows @wrapMethod(CClass) → MethodList

#[test]
fn after_wrap_method_stage1_offers_only_function_keyword() {
    // After the closing ')' only `function` should be offered — no method names yet.
    let source = concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@wrapMethod(CPlayer) \n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 3,
            character: 21,
        },
    );

    assert!(
        matches!(result, Some(AfterWrapMethodCompletions::FunctionKeyword)),
        "stage 1 must yield FunctionKeyword, got {result:?}"
    );
}

#[test]
fn after_wrap_method_stage1_none_for_unknown_class() {
    let source = "@wrapMethod(CUnknown) \n";
    let doc = make_doc(source);
    let ws = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&ws, &base);

    let result = after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 0,
            character: 22,
        },
    );

    assert!(result.is_none(), "unknown class should yield None");
}

#[test]
fn after_wrap_method_stage1_none_for_struct() {
    let source = concat!("struct SData {}\n", "@wrapMethod(SData) \n",);
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 19,
        },
    );

    assert!(result.is_none(), "struct target should yield None");
}

#[test]
fn after_wrap_method_stage2_offers_method_list_after_function_keyword() {
    // After `@wrapMethod(CPlayer)\nfunction ` the method list is offered.
    let source = concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "  public event OnDeath() {}\n",
        "  public var mHp : int;\n",
        "}\n",
        "@wrapMethod(CPlayer)\n",
        "function \n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 6,
            character: 9,
        },
    );

    let methods = match result {
        Some(AfterWrapMethodCompletions::MethodList(m)) => m,
        other => panic!("expected MethodList, got {other:?}"),
    };
    let names: Vec<&str> = methods.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains(&"OnSpawned"), "method should be offered");
    assert!(names.contains(&"OnDeath"), "event should be offered");
    assert!(!names.contains(&"mHp"), "field must not be offered");
}

#[test]
fn after_wrap_method_stage2_does_not_walk_inheritance() {
    let source_base = "class CBase {\n  public function BaseMethod() {}\n}\n";
    let source = concat!(
        "class CChild extends CBase {\n",
        "  public function OwnMethod() {}\n",
        "}\n",
        "@wrapMethod(CChild)\n",
        "function \n",
    );
    let doc_base = make_doc(source_base);
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///base.ws", &doc_base);
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let methods = match after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 9,
        },
    ) {
        Some(AfterWrapMethodCompletions::MethodList(m)) => m,
        other => panic!("expected MethodList, got {other:?}"),
    };

    let names: Vec<&str> = methods.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains(&"OwnMethod"), "own method should appear");
    assert!(
        !names.contains(&"BaseMethod"),
        "inherited method must not appear"
    );
}

#[test]
fn after_wrap_method_stage1_offers_function_keyword_while_typing_partial_word() {
    // Typing a partial word (e.g. "fun") as the first token after @wrapMethod(CPlayer)
    // must still yield FunctionKeyword — not MethodList, not None.
    let source = concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@wrapMethod(CPlayer)\n",
        "fun\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 3,
        },
    );

    assert!(
        matches!(result, Some(AfterWrapMethodCompletions::FunctionKeyword)),
        "typing a partial first word after @wrapMethod must yield FunctionKeyword, got {result:?}"
    );
}

#[test]
fn after_wrap_method_stage2_offers_method_list_while_typing_method_name() {
    // Cursor is partway through a method name after `@wrapMethod(CPlayer) function`.
    // The method list must be returned at any point within the partial name.
    let source = concat!(
        "class CPlayer {\n",
        "  public function AddMethodA() {}\n",
        "  public function AddMethodB() {}\n",
        "}\n",
        "@wrapMethod(CPlayer) function AddMet\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    // CST (from dump_tree): ident("AddMet") bytes 30..36
    // Test at three cursor positions within "AddMet".
    for character in [31u32, 33, 36] {
        let result =
            after_wrap_method_completions(&doc, &db, SourcePosition { line: 4, character });
        let methods = match result {
            Some(AfterWrapMethodCompletions::MethodList(m)) => m,
            other => panic!("expected MethodList at character {character}, got {other:?}"),
        };
        let names: Vec<&str> = methods.iter().map(|d| d.symbol.name.as_str()).collect();
        assert!(
            names.contains(&"AddMethodA"),
            "AddMethodA missing at char {character}"
        );
        assert!(
            names.contains(&"AddMethodB"),
            "AddMethodB missing at char {character}"
        );
    }
}

#[test]
fn after_wrap_method_stage2_none_when_function_not_preceded_by_wrap_method() {
    // `function ` at top level with no preceding @wrapMethod must not trigger.
    let source = "function \n";
    let doc = make_doc(source);
    let ws = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&ws, &base);

    let result = after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 0,
            character: 9,
        },
    );

    assert!(
        result.is_none(),
        "bare `function` should not trigger wrap method completions"
    );
}

#[test]
fn after_replace_method_stage1_offers_only_function_keyword() {
    let source = concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "}\n",
        "@replaceMethod(CPlayer) \n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 3,
            character: 24,
        },
    );

    assert!(
        matches!(result, Some(AfterWrapMethodCompletions::FunctionKeyword)),
        "stage 1 must yield FunctionKeyword for @replaceMethod, got {result:?}"
    );
}

#[test]
fn after_replace_method_stage2_offers_method_list() {
    let source = concat!(
        "class CPlayer {\n",
        "  public function OnSpawned() {}\n",
        "  public event OnDeath() {}\n",
        "  public var mHp : int;\n",
        "}\n",
        "@replaceMethod(CPlayer)\n",
        "function \n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let result = after_wrap_method_completions(
        &doc,
        &db,
        SourcePosition {
            line: 6,
            character: 9,
        },
    );

    let methods = match result {
        Some(AfterWrapMethodCompletions::MethodList(m)) => m,
        other => panic!("expected MethodList, got {other:?}"),
    };
    let names: Vec<&str> = methods.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains(&"OnSpawned"), "method should be offered");
    assert!(names.contains(&"OnDeath"), "event should be offered");
    assert!(!names.contains(&"mHp"), "field must not be offered");
}

// --- annotation_name_completions ---

#[test]
fn annotation_name_completions_gate() {
    for (label, source, line, character, fires) in [
        ("typing partial name @w", "@w\n", 0u32, 2u32, true),
        (
            "inside annotation parens",
            "@wrapMethod(CPlayer)\n",
            0,
            12,
            false,
        ),
        (
            "inside string literal",
            "function F() { var x : string = \"hello@world\"; }",
            0,
            39,
            false,
        ),
        // `@` at byte 27 (char 27) inside a malformed function (outer ERROR contains `{`).
        // Cursor at char 28. Gate must not fire even though outer ERROR is a direct child of script.
        (
            "inside function body",
            "function a(){var b:string=\"@",
            0,
            28,
            false,
        ),
        (
            "inside function body",
            "function a(){var b:string=\"@}",
            0,
            14,
            false,
        ),
        (
            "bare @ between class decls",
            "\nclass a{\n\t\n}\n@\nclass b{function c(){}}",
            4,
            1,
            true,
        ),
        // `a@` — outer ERROR has two children (ident + inner ERROR); must not fire.
        // Cursor at char 2 (byte 2, immediately after `@` at byte 1).
        ("identifier immediately before @", "a@", 0, 2, false),
    ] {
        let doc = make_doc(source);
        let result = annotation_name_completions(&doc, SourcePosition { line, character });
        if fires {
            assert!(result.is_some(), "{label}: expected gate to fire");
        } else {
            assert!(result.is_none(), "{label}: expected gate not to fire");
        }
    }
}

#[test]
fn annotation_name_completions_fires_on_bare_at_sign() {
    // Bare `@` parses as ERROR/ERROR (no annotation_ident child).
    // Cursor is at character 1 (byte 1, immediately after `@` at bytes 0..1).
    // Gate must still fire and return the position of `@` (line 0, character 0).
    let source = "@\n";
    let doc = make_doc(source);

    let at_pos = annotation_name_completions(
        &doc,
        SourcePosition {
            line: 0,
            character: 1,
        },
    );
    assert!(at_pos.is_some(), "should fire on bare @");
    let pos = at_pos.unwrap();
    assert_eq!(pos.line, 0, "@ position line");
    assert_eq!(pos.character, 0, "@ position character");
}

// --- completions inside annotated function bodies ---

fn index_docs(docs: &[(&str, &crate::document::ParsedDocument)]) -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();
    for (uri, doc) in docs {
        index.update_document(*uri, doc);
    }
    index
}

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
