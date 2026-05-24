use super::super::{after_wrap_method_completions, AfterWrapMethodCompletions};
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;

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
