use super::super::{
    class_header_keyword_completions, completion_members, extends_completions,
    state_owner_completions, type_completions,
};
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;

#[test]
fn type_completions_offered_right_after_colon_no_type_yet() {
    // Cursor immediately after `:` — tree-sitter cannot produce a type_annot
    // node yet (ERROR recovery), so the old ancestor check returns None.
    let source = "class CTest {}\nclass C {var test:}";
    // line 1: "class C {var test:}"
    //          0         1
    //          0123456789012345678
    //                            ^ col 18 = right after ':'
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let types = type_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 18,
        },
    );
    assert!(
        !types.is_empty(),
        "type completions must be offered right after ':' even before any type name is typed"
    );
}

#[test]
fn type_completions_offered_in_type_annotation() {
    // "var x : CP" with a complete statement on the next line gives tree-sitter
    // enough context to recover and emit a type_annot node for the partial name.
    let source = concat!(
        "class CPlayer {}\n",
        "struct SData {}\n",
        "enum EDir { North = 0 }\n",
        "function Test() {\n",
        "  var x : CP\n",
        "  var y : int;\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let types = type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 4,
            character: 11,
        },
    );

    let names: Vec<&str> = types.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CPlayer"),
        "class should be in type completions"
    );
    assert!(
        names.contains(&"SData"),
        "struct should be in type completions"
    );
    assert!(
        names.contains(&"EDir"),
        "enum should be in type completions"
    );
}

#[test]
fn type_completions_not_offered_inside_string_literal() {
    // The unterminated string causes an ERROR node — no type_annot ancestor exists,
    // so completions must not fire. CPlayer is indexed to prove the guard is what
    // suppresses it, not an empty type list.
    let source = concat!("class CPlayer {}\n", "function SomeFunc(\"test:\n",);
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let types = type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 24,
        },
    );

    assert!(
        types.is_empty(),
        "colon inside a string literal must not trigger type completion"
    );
}

#[test]
fn type_completions_not_offered_outside_type_context() {
    let source = "function Test() {\n  someVar\n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();

    let types = type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 9,
        },
    );

    assert!(
        types.is_empty(),
        "no type completions outside a type annotation"
    );
}

#[test]
fn type_completions_offered_cursor_right_of_complete_type_name() {
    // Regression: cursor positioned after a complete type name must still offer
    // completions. The byte offset lands on ';'; the type name is found via the -1 fallback.
    let source = "class CMyType {}\nfunction F() {\n  var z:CMyType;\n  var w : int;\n}\n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let types = type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 15,
        },
    );
    assert!(
        !types.is_empty(),
        "cursor right of a complete type name must still offer completions"
    );
}

#[test]
fn type_completions_offered_cursor_right_of_last_type_in_error_recovery() {
    // "var z : A : B : CMyType;" is a syntax error — tree-sitter only produces a
    // type_annot node for the final ": CMyType"; the earlier ": A" and ": B" become
    // ERROR nodes. Completions must still work at and after "CMyType".
    let source =
        "class CMyType {}\nfunction F() {\n  var z : A : B : CMyType;\n  var w : int;\n}\n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let types_at = type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 18,
        },
    );
    assert!(
        !types_at.is_empty(),
        "cursor at the start of the final type name must offer completions"
    );

    let types_after = type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 25,
        },
    );
    assert!(
        !types_after.is_empty(),
        "cursor right of the final type name must offer completions"
    );
}

#[test]
fn field_type_annotation_in_class_body_fires_type_completions() {
    let source = include_str!("../../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 15: start of CRefType in `  var isName : CRefType;`
    let pos = SourcePosition {
        line: 3,
        character: 15,
    };

    let members = completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a field type annotation"
    );

    let types = type_completions(&doc, &db, pos);
    let type_names: Vec<&str> = types.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        type_names.contains(&"CRefType"),
        "CRefType must appear in type completions at the field type position"
    );
}

#[test]
fn field_name_between_methods_yields_no_completions() {
    let source = include_str!("../../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 6: start of someVar in `  var someVar : int;` — between SomeFunction and Name
    let pos = SourcePosition {
        line: 5,
        character: 6,
    };

    let members = completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a variable name declaration"
    );

    let types = type_completions(&doc, &db, pos);
    assert!(
        types.is_empty(),
        "type completions must not fire at a variable name (not a type annotation)"
    );
}

#[test]
fn field_type_between_methods_fires_type_completions() {
    let source = include_str!("../../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 16: start of int in `  var someVar : int;` — between SomeFunction and Name
    let pos = SourcePosition {
        line: 5,
        character: 16,
    };

    let members = completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a field type annotation"
    );

    let types = type_completions(&doc, &db, pos);
    assert!(
        !types.is_empty(),
        "type completions must fire at the field type position"
    );
}

#[test]
fn parameter_type_annotation_fires_type_completions() {
    // `CParam` is in the parameter type annotation — must trigger type completions
    // regardless of whether the enclosing callable is a free function or a method.
    let source = concat!("class CParam {}\n", "function Foo(x : CParam) {}\n",);
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 1: `function Foo(x : CParam) {}`
    //          0         1
    //          0123456789012345678
    //                            ^ col 17 = start of CParam
    let pos = SourcePosition {
        line: 1,
        character: 17,
    };

    let members = completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a parameter type annotation"
    );

    let types = type_completions(&doc, &db, pos);
    let type_names: Vec<&str> = types.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        type_names.contains(&"CParam"),
        "CParam must appear in type completions at the parameter type position"
    );
}

#[test]
fn function_return_type_annotation_fires_type_completions() {
    let source = concat!(
        "class CReturnType {}\n",
        "function Foo() : CReturnType {}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 1: `function Foo() : CReturnType {}`
    //          0         1
    //          01234567890123456789
    //                             ^ col 17 = start of CReturnType
    let pos = SourcePosition {
        line: 1,
        character: 17,
    };

    let members = completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a return type annotation"
    );

    let types = type_completions(&doc, &db, pos);
    let type_names: Vec<&str> = types.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        type_names.contains(&"CReturnType"),
        "CReturnType must appear in type completions at the return type position"
    );
}

#[test]
fn extends_completions_fires_after_extends_keyword_incomplete_decl() {
    // Source: class with no body — whole decl is an ERROR node in the tree.
    // Cursor is right after the space following 'extends'.
    let source = "class CExample {}\nclass Foo extends \n";
    //            line 0              line 1: "class Foo extends " (chars 0-17, cursor at 18)
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = extends_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 18,
        },
    );
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CExample"),
        "class names must appear in extends completions after the 'extends' keyword"
    );
}

#[test]
fn extends_completions_fires_mid_base_class_name() {
    // Cursor is inside a partially-typed base class name — still an ERROR node.
    let source = "class CExample {}\nclass Foo extends CEx\n";
    //            line 1: "class Foo extends CEx" — cursor at char 20 (on 'x')
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = extends_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 20,
        },
    );
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CExample"),
        "extends completions must fire while mid-typing the base class name"
    );
}

#[test]
fn extends_completions_empty_inside_class_body() {
    // Cursor is inside the class body — must NOT trigger extends completions.
    let source = "class CExample {}\nclass Foo extends CExample {\n  \n}\n";
    //            line 2: "  " — cursor at char 2 (inside body)
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = extends_completions(
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    assert!(
        result.is_empty(),
        "extends completions must not fire inside the class body"
    );
}

#[test]
fn extends_completions_empty_at_class_name_position() {
    // Cursor is on the class name itself — no 'extends' keyword present.
    let source = "class Foo {\n  \n}\n";
    //            line 0: "class Foo {" — cursor at char 6 (on 'F')
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = extends_completions(
        &doc,
        &db,
        SourcePosition {
            line: 0,
            character: 6,
        },
    );
    assert!(
        result.is_empty(),
        "extends completions must not fire at the class name position"
    );
}

#[test]
fn extends_completions_for_state_decl_returns_states_only() {
    // `state X in Owner extends ` should offer states (other states to extend),
    // not classes — a state can only extend another state.
    let source = concat!(
        "class CBase {}\n",
        "state BaseState in CBase {}\n",
        "state IdleState in CBase extends \n",
    );
    //            line 2: "state IdleState in CBase extends " (32 chars, cursor at 32)
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = extends_completions(
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 32,
        },
    );
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"BaseState"),
        "state extends must offer other states as base"
    );
    assert!(
        !names.contains(&"CBase"),
        "state extends must not offer classes"
    );
}

#[test]
fn extends_completions_excludes_enums_and_structs() {
    // Only Class and State symbols must appear — not Enum or Struct.
    let source = concat!(
        "class CExample {}\n",
        "struct SExample {}\n",
        "enum EExample {}\n",
        "class Foo extends \n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 3: "class Foo extends " — cursor at char 18
    let result = extends_completions(
        &doc,
        &db,
        SourcePosition {
            line: 3,
            character: 18,
        },
    );
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CExample"),
        "Class symbols must appear in extends completions"
    );
    assert!(
        !names.contains(&"SExample"),
        "Struct symbols must not appear in extends completions"
    );
    assert!(
        !names.contains(&"EExample"),
        "Enum symbols must not appear in extends completions"
    );
}

#[test]
fn extends_completions_empty_between_extends_and_class_body() {
    // "class C extends  {}" — two spaces between 'extends' and '{'
    // Cursor is in the second space (line 1, char 16). No base class name has been typed,
    // but the body {} is already present. Must not fire completions.
    let source = "class CExample {}\nclass C extends  {}\n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = extends_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 16,
        },
    );
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CExample"),
        "extends completions must fire when cursor is in whitespace between 'extends' and the class body, so the user can pick a base class"
    );
}

// --- class_header_keyword_completions ---

#[test]
fn header_kw_offers_extends_after_class_name() {
    // `class Foo ` with trailing space and no body → offer `extends`.
    let source = "class Foo \n";
    let doc = make_doc(source);
    let result = class_header_keyword_completions(
        &doc,
        SourcePosition {
            line: 0,
            character: 10,
        },
    );
    assert_eq!(
        result,
        vec!["extends"],
        "class with name and no body should offer the extends keyword"
    );
}

#[test]
fn header_kw_offers_in_after_state_name() {
    // `state Foo ` with trailing space → offer `in`.
    let source = "state Foo \n";
    let doc = make_doc(source);
    let result = class_header_keyword_completions(
        &doc,
        SourcePosition {
            line: 0,
            character: 10,
        },
    );
    assert_eq!(
        result,
        vec!["in"],
        "state with name and no parent should offer the in keyword"
    );
}

#[test]
fn header_kw_offers_extends_after_state_owner() {
    // `state Foo in Bar ` → offer `extends`.
    let source = "state Foo in Bar \n";
    let doc = make_doc(source);
    let result = class_header_keyword_completions(
        &doc,
        SourcePosition {
            line: 0,
            character: 17,
        },
    );
    assert_eq!(
        result,
        vec!["extends"],
        "state with owner and no base should offer the extends keyword"
    );
}

#[test]
fn header_kw_empty_inside_class_body() {
    let source = "class Foo {\n  \n}\n";
    let doc = make_doc(source);
    let result = class_header_keyword_completions(
        &doc,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    assert!(result.is_empty(), "must not fire inside class body");
}

#[test]
fn header_kw_empty_at_top_level_blank() {
    let source = "\n";
    let doc = make_doc(source);
    let result = class_header_keyword_completions(
        &doc,
        SourcePosition {
            line: 0,
            character: 0,
        },
    );
    assert!(
        result.is_empty(),
        "must not fire when no class/state header is in progress"
    );
}

#[test]
fn header_kw_empty_when_extends_already_typed() {
    // `class Foo extends ` — past the extends keyword; the extends-target completion
    // should handle this slot, not the header-keyword completion.
    let source = "class Foo extends \n";
    let doc = make_doc(source);
    let result = class_header_keyword_completions(
        &doc,
        SourcePosition {
            line: 0,
            character: 18,
        },
    );
    assert!(
        result.is_empty(),
        "must not re-offer 'extends' once it has been typed"
    );
}

// --- state_owner_completions ---

#[test]
fn state_owner_offers_classes_after_in() {
    // `state Foo in ` — offer classes (not states/structs/enums).
    let source = concat!(
        "class COwner {}\n",
        "state SBase in COwner {}\n",
        "struct SStruct {}\n",
        "enum EEnum {}\n",
        "state Foo in \n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = state_owner_completions(
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 13,
        },
    );
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"COwner"),
        "state owner slot must offer classes"
    );
    assert!(
        !names.contains(&"SBase"),
        "state owner slot must not offer states"
    );
    assert!(
        !names.contains(&"SStruct"),
        "state owner slot must not offer structs"
    );
    assert!(
        !names.contains(&"EEnum"),
        "state owner slot must not offer enums"
    );
}

#[test]
fn state_owner_empty_after_owner_typed() {
    // After the owner ident is fully typed, this completion no longer fires.
    let source = "class COwner {}\nstate Foo in COwner \n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = state_owner_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 20,
        },
    );
    assert!(result.is_empty());
}

#[test]
fn state_owner_empty_inside_class_extends_slot() {
    // `class Foo extends ` is the class extends slot — state_owner_completions
    // must not fire here.
    let source = "class CBase {}\nclass Foo extends \n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = state_owner_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 18,
        },
    );
    assert!(result.is_empty());
}
