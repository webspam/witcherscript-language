use super::super::{
    after_wrap_method_completions, annotation_arg_completions, AfterWrapMethodCompletions,
};
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;

#[test]
fn annotation_arg_completions_offers_classes_inside_parens() {
    // Use a complete annotation so tree-sitter produces a well-formed annotation
    // node with explicit '(' and ')' children. Cursor sits on the 'C' of 'CPlayer'.
    let source = concat!(
        "class CPlayer {}\n",
        "struct SData {}\n",
        "enum EDir { North = 0 }\n",
        "@wrapMethod(CPlayer)\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let completions = annotation_arg_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        // character 12 is on the 'C' of 'CPlayer', inside the parens
        SourcePosition {
            line: 3,
            character: 12,
        },
    );

    let names: Vec<&str> = completions.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CPlayer"),
        "class should be offered inside annotation parens"
    );
    assert!(
        !names.contains(&"SData"),
        "struct should not be offered inside annotation parens"
    );
    assert!(
        !names.contains(&"EDir"),
        "enum should not be offered inside annotation parens"
    );
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
