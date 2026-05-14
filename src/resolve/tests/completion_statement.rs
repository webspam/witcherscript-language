use super::super::{expression_completions, statement_completions};
use super::{make_doc, make_env, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

#[test]
fn statement_completions_excludes_local_declared_after_cursor() {
    let source = "function Test() {\n  var bar : int;\n  bar;\n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor at line 1, character 2 — before the `bar` identifier in the declaration
    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    let local_names: Vec<&str> = result
        .locals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        !local_names.contains(&"bar"),
        "variable declared after cursor must not appear in locals"
    );
}

#[test]
fn statement_completions_includes_local_declared_before_cursor() {
    let source = "function Test() {\n  var count : int;\n  count;\n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor at line 2, character 2 — after the `count` declaration on line 1
    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    let local_names: Vec<&str> = result
        .locals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        local_names.contains(&"count"),
        "variable declared before cursor must appear in locals"
    );
}

#[test]
fn statement_completions_includes_parameters() {
    let source = "function Test(owner : int) {\n  owner;\n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    let local_names: Vec<&str> = result
        .locals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        local_names.contains(&"owner"),
        "function parameter must appear in locals"
    );
    assert!(
        result
            .locals
            .iter()
            .any(|d| d.symbol.name == "owner" && d.symbol.kind == SymbolKind::Parameter),
        "owner must have kind Parameter"
    );
}

#[test]
fn statement_completions_members_includes_private_symbols_of_own_class() {
    let source = concat!(
        "class CExample {\n",
        "  private var secret : int;\n",
        "  private function Hidden() {}\n",
        "  function Test() {\n",
        "    secret;\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor on line 4, character 4 — inside the Test method body
    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 4,
        },
    );
    let member_names: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"secret"),
        "private field should appear in members when inside the class"
    );
    assert!(
        member_names.contains(&"Hidden"),
        "private method should appear in members when inside the class"
    );
}

#[test]
fn statement_completions_members_empty_in_free_function() {
    let source = "function Test() {\n  \n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    assert!(
        result.members.is_empty(),
        "members bucket must be empty when cursor is in a free function"
    );
}

#[test]
fn statement_completions_globals_contains_functions_from_indexed_documents() {
    let doc_a = make_doc("function Alpha() {}\n");
    let doc_b = make_doc("function Beta() {}\n");
    let doc_c = make_doc("function Caller() {\n  \n}\n");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);
    index.update_document("file:///c.ws", &doc_c);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///c.ws",
        &doc_c,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    let global_names: Vec<&str> = result
        .globals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        global_names.contains(&"Alpha"),
        "Alpha from another document must appear in globals"
    );
    assert!(
        global_names.contains(&"Beta"),
        "Beta from another document must appear in globals"
    );
}

#[test]
fn statement_completions_globals_includes_script_env_globals() {
    let doc = make_doc("function Caller() {\n  \n}\n");
    let env = make_env("theGame", "CR4Game");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base).with_script_env(&env);

    let result = statement_completions(
        "file:///c.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    let global = result
        .globals
        .iter()
        .find(|d| d.symbol.name == "theGame")
        .expect("script env global must appear in statement completions");
    assert_eq!(global.symbol.kind, SymbolKind::Variable);
    assert_eq!(global.symbol.type_annotation.as_deref(), Some("CR4Game"));
}

#[test]
fn expression_completions_globals_includes_script_env_globals() {
    let doc = make_doc("function Caller() : int {\n  return \n}\n");
    let env = make_env("theGame", "CR4Game");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base).with_script_env(&env);

    let result = expression_completions(
        "file:///c.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 9,
        },
    )
    .expect("expression completions should fire after `return `");
    assert!(
        result.globals.iter().any(|d| d.symbol.name == "theGame"),
        "script env global must appear in expression completions"
    );
}

#[test]
fn statement_completions_globals_excludes_class_methods() {
    let source = concat!(
        "class Foo {\n",
        "  function Bar() {}\n",
        "}\n",
        "function Outer() {\n",
        "  \n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 2,
        },
    );
    let global_names: Vec<&str> = result
        .globals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        !global_names.contains(&"Bar"),
        "class method Bar must not appear in globals"
    );
}

#[test]
fn statement_completions_all_empty_outside_any_callable() {
    let source = "class CExample {}\n\n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor at line 1, character 0 — between definitions, not inside any callable
    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 0,
        },
    );
    assert!(
        result.locals.is_empty() && result.members.is_empty() && result.globals.is_empty(),
        "all buckets must be empty when cursor is outside any callable"
    );
}

#[test]
fn statement_completions_members_includes_inherited_public_method() {
    let source_b = "class B {\n  public function BMethod() {}\n}\n";
    let source_a = "class A extends B {\n  function Test() {\n    \n  }\n}\n";
    let doc_b = make_doc(source_b);
    let doc_a = make_doc(source_a);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///b.ws", &doc_b);
    index.update_document("file:///a.ws", &doc_a);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor at line 2, character 4 — inside A::Test method body
    let result = statement_completions(
        "file:///a.ws",
        &doc_a,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    );
    let member_names: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"BMethod"),
        "inherited public method from parent class must appear in members"
    );
}

#[test]
fn statement_completions_globals_excludes_exec_and_quest_functions() {
    let source = concat!(
        "exec function DebugCmd() {}\n",
        "quest function QuestFunc() {}\n",
        "function NormalFunc() {}\n",
        "function Caller() {\n",
        "  \n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 2,
        },
    );
    let global_names: Vec<&str> = result
        .globals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        !global_names.contains(&"DebugCmd"),
        "exec function must not appear in globals"
    );
    assert!(
        !global_names.contains(&"QuestFunc"),
        "quest function must not appear in globals"
    );
    assert!(
        global_names.contains(&"NormalFunc"),
        "normal function must still appear in globals"
    );
}

#[test]
fn statement_completions_has_this_inside_class_method() {
    let source = "class CExample {\n  function Test() {\n    \n  }\n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    );
    assert!(
        result.has_this,
        "this must be available inside a class method"
    );
    assert!(
        !result.has_super,
        "super must not be available without a superclass"
    );
}

#[test]
fn statement_completions_has_super_when_class_extends() {
    let source_b = "class B {}\n";
    let source_a = "class A extends B {\n  function Test() {\n    \n  }\n}\n";
    let doc_b = make_doc(source_b);
    let doc_a = make_doc(source_a);

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///b.ws", &doc_b);
    index.update_document("file:///a.ws", &doc_a);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///a.ws",
        &doc_a,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    );
    assert!(
        result.has_this,
        "this must be available inside a class method"
    );
    assert!(
        result.has_super,
        "super must be available when class extends another"
    );
}

#[test]
fn statement_completions_no_this_in_free_function() {
    let source = "function Test() {\n  \n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    assert!(
        !result.has_this,
        "this must not be available in a free function"
    );
    assert!(
        !result.has_super,
        "super must not be available in a free function"
    );
}

#[test]
fn statement_completions_empty_after_dot_in_class_method() {
    // Regression: typing `someVar.` inside a class method must not trigger
    // statement completions — that belongs to completion_members.
    let source = concat!(
        "class CExample {\n",
        "  var mField : int;\n",
        "  function Test() {\n",
        "    var local : CExample;\n",
        "    local.\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor right after the dot on line 4 ("    local." = chars 0-9, dot at 9, cursor at 10).
    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 10,
        },
    );
    assert!(
        result.locals.is_empty()
            && result.members.is_empty()
            && result.globals.is_empty()
            && !result.has_this
            && !result.has_super,
        "dot-access context must not produce statement completions"
    );
}

#[test]
fn statement_completions_in_switch_true_inside_switch() {
    let source = include_str!("../../../tests/fixtures/valid/switch_stmt.ws");
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    for (line, label) in [
        // start of the `case 3:` line; prev token is `;` from `break;`
        (7, "switch body level after semicolon"),
        // blank line after fall-through `case 1:`; prev token is `:`
        (4, "blank line after fall-through case label"),
    ] {
        let result = statement_completions(
            "file:///test.ws",
            &doc,
            &db,
            SourcePosition { line, character: 0 },
        );
        assert!(result.in_switch, "in_switch must be true at {label}");
    }
}

#[test]
fn statement_completions_in_switch_false_inside_nested_block() {
    // switch_stmt.ws line 9:0 — blank line inside the if body, nested inside
    // the switch_block. Nearest enclosing block is func_block, not switch_block.
    let source = include_str!("../../../tests/fixtures/valid/switch_stmt.ws");
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 9,
            character: 0,
        },
    );
    assert!(
        !result.in_switch,
        "in_switch must be false inside a nested block within a switch"
    );
}

#[test]
fn statement_completions_in_switch_false_outside_switch() {
    let source = "function Test() {\n  \n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    assert!(
        !result.in_switch,
        "in_switch must be false in a plain function body"
    );
}

#[test]
fn statement_completions_offered_after_if_condition() {
    // if_stmt.ws:
    //   line 2: `  if (x > 0)`       — braceless; prev token at (3,0) is `)` from if_stmt
    //   line 4: `  if (x > 0) {`     — braced; prev token at (5,0) is `{`
    let source = include_str!("../../../tests/fixtures/valid/if_stmt.ws");
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    for (line, character, label) in [
        // start of `x = 1;`; prev is `)` closing braceless if condition
        (3, 0, "braceless if body, next-line statement"),
        // blank line inside braced if body; prev is `{`
        (5, 0, "braced if body"),
        // `return` keyword on same line as if; prev before its start is `)` closing if condition
        (7, 24, "braceless if body, same-line return"),
    ] {
        let result = statement_completions(
            "file:///test.ws",
            &doc,
            &db,
            SourcePosition { line, character },
        );
        let local_names: Vec<&str> = result
            .locals
            .iter()
            .map(|d| d.symbol.name.as_str())
            .collect();
        assert!(
            local_names.contains(&"x"),
            "{label}: local `x` must be visible (statement completions must fire)"
        );
    }
}

#[test]
fn statement_completions_offered_after_a_comment() {
    // A comment before the cursor must not hide the real boundary (the `{`).
    let source = "function Test(owner : int) {\n  // a note\n  \n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 2, character 2 — blank line directly after the comment line
    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    let local_names: Vec<&str> = result
        .locals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        local_names.contains(&"owner"),
        "a comment before the cursor must not suppress statement completions"
    );
}

#[test]
fn statement_completions_in_loop_true_inside_loop_bodies() {
    let source = include_str!("../../../tests/fixtures/valid/loop_stmts.ws");
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    for (line, label) in [
        // blank line inside for body
        (3, "for body"),
        // blank line inside while body
        (6, "while body"),
        // blank line inside do body
        (9, "do-while body"),
        // blank line inside if nested in a for loop; break/continue must still be offered
        (13, "if nested within a for loop"),
    ] {
        let result = statement_completions(
            "file:///test.ws",
            &doc,
            &db,
            SourcePosition { line, character: 0 },
        );
        assert!(result.in_loop, "in_loop must be true inside {label}");
    }
}

#[test]
fn statement_completions_in_loop_false_outside_loop() {
    let source = "function Test() {\n  \n}\n";
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    assert!(
        !result.in_loop,
        "in_loop must be false in a plain function body"
    );
}

#[test]
fn statement_completions_empty_after_leading_dot_in_method() {
    // A bare '.' at the start of a statement has no valid LHS — tree-sitter
    // produces an incomplete_member_access_expr with a missing receiver.
    // statement_completions must not fire here.
    let source = "class C {\n  function A() {\n    .\n  }\n}\n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 2: "    ." — dot at col 4, cursor at col 5 (right after the dot)
    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 5,
        },
    );
    assert!(
        result.locals.is_empty()
            && result.members.is_empty()
            && result.globals.is_empty()
            && !result.has_this
            && !result.has_super,
        "a leading dot with no LHS must not produce statement completions"
    );
}

// --- Completion context detection tests ---
//
// Fixture layout for completion_class_body_contexts.ws:
//   line 0:  class C {
//   line 1:    var field : int;
//   line 2:    (blank)                    ← class body, outside any callable
//   line 3:    function Name(test : bool) {
//   line 4:    (blank)                    ← method body, inside callable
//   line 5:    }
//   line 6:  }
//
// Fixture layout for completion_declaration_contexts.ws:
//   line 0:  class CRefType {}
//   line 1:  (blank)
//   line 2:  class C {
//   line 3:    var isName : CRefType;     col 15 = start of CRefType (field type)
//   line 4:    private function SomeFunction() {}
//                                         col 19 = start of SomeFunction (fn name decl)
//   line 5:    var someVar : int;         col  6 = start of someVar (var name, between callables)
//                                         col 16 = start of int (field type)
//   line 6:    function Name(test : int, other : bool) {}
//                                         col 16 = start of test  (1st param name)
//                                         col 23 = start of int   (1st param type)
//                                         col 28 = start of other (2nd param name)
//                                         col 36 = start of bool  (2nd param type)
//   line 7:  }

#[test]
fn blank_in_class_body_yields_no_completions() {
    let source = include_str!("../../../tests/fixtures/valid/completion_class_body_contexts.ws");
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 2,
        character: 0,
    };

    let members = super::super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire on a blank line in the class body"
    );

    let types = super::super::type_completions(&doc, &db, pos);
    assert!(
        types.is_empty(),
        "type completions must not fire on a blank line in the class body"
    );

    let stmt = statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty()
            && !stmt.has_this
            && !stmt.has_super,
        "statement completions must be all-empty in the class body (no enclosing callable)"
    );
}

#[test]
fn blank_in_class_method_body_yields_statement_completions() {
    let source = include_str!("../../../tests/fixtures/valid/completion_class_body_contexts.ws");
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 4,
        character: 0,
    };

    let members = super::super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire without a dot"
    );

    let types = super::super::type_completions(&doc, &db, pos);
    assert!(
        types.is_empty(),
        "type completions must not fire without a type annotation"
    );

    let stmt = statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.has_this,
        "this must be available inside a class method body"
    );
    let local_names: Vec<&str> = stmt.locals.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        local_names.contains(&"test"),
        "function parameter 'test' must appear in locals at the blank line"
    );
    let member_names: Vec<&str> = stmt
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"field"),
        "class field 'field' must appear in members inside the method"
    );
}

#[test]
fn function_name_in_class_body_yields_no_statement_completions() {
    let source = include_str!("../../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 19: start of SomeFunction in `  private function SomeFunction() {}`
    let pos = SourcePosition {
        line: 4,
        character: 19,
    };

    let stmt = statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty()
            && !stmt.has_this,
        "statement completions must be all-empty at a function name declaration"
    );
}

#[test]
fn parameter_name_yields_no_statement_completions() {
    let source = include_str!("../../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 16: start of `test`  (first parameter name)
    // col 28: start of `other` (second parameter name, after the comma)
    for (label, character) in [("first param", 16u32), ("second param after comma", 28u32)] {
        let pos = SourcePosition { line: 6, character };
        let stmt = statement_completions("file:///test.ws", &doc, &db, pos);
        assert!(
            stmt.locals.is_empty()
                && stmt.members.is_empty()
                && stmt.globals.is_empty()
                && !stmt.has_this,
            "statement completions must be all-empty at parameter name ({label})"
        );
    }
}

// ── var-name position completions ───────────────────────────────────────────
//
// Invariant: completions must NOT fire when cursor is on the identifier being
// declared as a new variable name.  They MUST fire for any other position in
// the function body (bare identifier expressions, position after 'var' keyword,
// etc.).
//
// CST observations (from dump_tree):
//   "class C { function Foo(p : int) { v } }"
//     → func_block > ERROR [ident(v)]               — bytes 34..35
//   "class C { function Foo(p : int) { var } }"
//     → func_block > ERROR [var]                    — var bytes 34..37
//   "class C { function Foo(p : int) { var x } }"
//     → func_block > ERROR [var, ident(x)]          — ident bytes 38..39
//   "class C { function Foo(p : int) { var x : int } }"
//     → func_block > local_var_decl_stmt [var, ident(x:names), ...]
//                    (MISSING semicolon)             — ident bytes 38..39

#[test]
fn incomplete_ident_expr_in_method_body_gets_statement_completions() {
    // "class C { function Foo(p : int) { v } }" — `v` at bytes 34..35
    // CST: ERROR [ident(v)] — only an ident inside ERROR, not a var declaration.
    // Completions must fire: this is an incomplete identifier reference, not a name being declared.
    let source = "class C { function Foo(p : int) { v } }";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let stmt = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 0,
            character: 35,
        },
    );
    assert!(
        stmt.has_this,
        "statement completions must fire for an incomplete identifier expression in a class method body"
    );
}

#[test]
fn var_keyword_alone_in_method_body_gets_statement_completions() {
    // "class C { function Foo(p : int) { var } }" — var at bytes 34..37
    // CST: ERROR [var] — only the keyword, no name typed yet.
    // Completions must fire: the user hasn't started naming a variable yet.
    let source = "class C { function Foo(p : int) { var } }";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 36 = 'r' of 'var' — cursor is inside the ERROR-wrapped keyword.
    let stmt = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 0,
            character: 36,
        },
    );
    assert!(
        stmt.has_this,
        "statement completions must fire when cursor is on the var keyword before any name is typed"
    );
}

#[test]
fn space_after_var_keyword_no_statement_completions() {
    // "class A { function N() { var }}" — cursor in the space at byte 28,
    // between `var` (bytes 25..28) and `}` (byte 29).
    // CST: ERROR [var] — keyword only, no name started.
    // Completions (this, super, globals) must be available at this position.
    let source = "class A { function N() { var }}";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let stmt = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 0,
            character: 29,
        },
    );
    assert!(
        !stmt.has_this
            && stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty(),
        "statement completions must not fire in the space after `var` — the user is about to declare a new name"
    );
}

#[test]
fn var_name_in_error_state_no_statement_completions() {
    // "class C { function Foo(p : int) { var x } }" — ident `x` at bytes 38..39
    // CST: ERROR [var, ident(x)] — incomplete var decl, no type annotation yet.
    // Completions must NOT fire: cursor is on the name being declared.
    let source = "class C { function Foo(p : int) { var x } }";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let stmt = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 0,
            character: 38,
        },
    );
    assert!(
        !stmt.has_this
            && stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty(),
        "statement completions must not fire at the name in an incomplete var declaration (ERROR state)"
    );
}

#[test]
fn local_var_name_in_method_body_yields_no_completions() {
    // `    var localName : int;` is on line 11 (0-indexed) inside MethodBody::DoSomething.
    // The fixture has MethodBody added at the bottom.
    // col 8: start of `localName` — declaring a new symbol, not referencing one.
    // CST: local_var_decl_stmt (complete, valid node, names field contains `localName`).
    let source = include_str!("../../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 11,
        character: 8,
    };

    let members = super::super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a local var name declaration"
    );

    let types = super::super::type_completions(&doc, &db, pos);
    assert!(
        types.is_empty(),
        "type completions must not fire at a local var name (not a type annotation)"
    );

    let stmt = statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty()
            && !stmt.has_this,
        "statement completions must be all-empty when declaring a new local variable name"
    );
}
