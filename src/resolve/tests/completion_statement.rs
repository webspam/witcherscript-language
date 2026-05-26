use rstest::rstest;

use super::super::{
    completion_members, expression_completions, merged_global_completions, statement_completions,
    type_completions, StatementCompletions,
};
use super::make_env;
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;
use crate::test_support::{def_names, TestDb};

#[derive(Clone, Copy)]
enum Bucket {
    Locals,
    Members,
    Globals,
}

fn names<'a>(
    r: &'a StatementCompletions,
    bucket: Bucket,
    globals: &'a [super::super::Definition],
) -> Vec<&'a str> {
    match bucket {
        Bucket::Locals => def_names(&r.locals),
        Bucket::Members => def_names(&r.members),
        Bucket::Globals => def_names(globals),
    }
}

fn run_at_cursor(fixture: &str) -> (TestDb, StatementCompletions) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let r = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
    (t, r)
}

fn globals_for_stmt(t: &TestDb, r: &StatementCompletions) -> Vec<super::super::Definition> {
    if r.needs_globals {
        merged_global_completions(&t.db())
    } else {
        Vec::new()
    }
}

#[rstest]
#[case::local_declared_after_cursor_excluded(
    "function Test() {\n  $0var bar : int;\n  bar;\n}\n",
    Bucket::Locals, &[], &["bar"],
)]
#[case::local_declared_before_cursor_included(
    "function Test() {\n  var count : int;\n  $0count;\n}\n",
    Bucket::Locals, &["count"], &[],
)]
#[case::parameter_appears_in_locals(
    "function Test(owner : int) {\n  $0owner;\n}\n",
    Bucket::Locals, &["owner"], &[],
)]
#[case::private_class_members_visible_inside_class(
    "class CExample {\n  private var secret : int;\n  private function Hidden() {}\n  function Test() {\n    $0secret;\n  }\n}\n",
    Bucket::Members, &["secret", "Hidden"], &[],
)]
#[case::cross_document_globals(
    "//- /a.ws\n\
     function Alpha() {}\n\
     //- /b.ws\n\
     function Beta() {}\n\
     //- /c.ws\n\
     function Caller() {\n  $0\n}\n",
    Bucket::Globals, &["Alpha", "Beta"], &[],
)]
#[case::class_methods_excluded_from_globals(
    "class Foo {\n  function Bar() {}\n}\nfunction Outer() {\n  $0\n}\n",
    Bucket::Globals, &[], &["Bar"],
)]
#[case::inherited_public_method_in_members(
    "//- /b.ws\n\
     class B {\n  public function BMethod() {}\n}\n\
     //- /a.ws\n\
     class A extends B {\n  function Test() {\n    $0\n  }\n}\n",
    Bucket::Members, &["BMethod"], &[],
)]
#[case::exec_quest_excluded_normal_included(
    "exec function DebugCmd() {}\nquest function QuestFunc() {}\nfunction NormalFunc() {}\nfunction Caller() {\n  $0\n}\n",
    Bucket::Globals, &["NormalFunc"], &["DebugCmd", "QuestFunc"],
)]
#[case::comment_before_cursor_does_not_hide_locals(
    "function Test(owner : int) {\n  // a note\n  $0\n}\n",
    Bucket::Locals, &["owner"], &[],
)]
fn statement_completions_membership(
    #[case] fixture: &str,
    #[case] bucket: Bucket,
    #[case] expected: &[&str],
    #[case] excluded: &[&str],
) {
    let (t, r) = run_at_cursor(fixture);
    let globals = globals_for_stmt(&t, &r);
    let names = names(&r, bucket, &globals);
    for n in expected {
        assert!(names.contains(n), "expected {n:?} in {names:?}");
    }
    for n in excluded {
        assert!(!names.contains(n), "excluded {n:?} appeared in {names:?}");
    }
}

#[rstest]
#[case::class_method_no_explicit_extends_defaults_to_cobject(
    "class CExample {\n  function Test() {\n    $0\n  }\n}\n",
    true,
    true
)]
#[case::extends_class_has_both(
    "//- /b.ws\n\
     class B {}\n\
     //- /a.ws\n\
     class A extends B {\n  function Test() {\n    $0\n  }\n}\n",
    true,
    true
)]
#[case::free_function_has_neither("function Test() {\n  $0\n}\n", false, false)]
fn statement_completions_this_super_flags(
    #[case] fixture: &str,
    #[case] expected_this: bool,
    #[case] expected_super: bool,
) {
    let (_t, r) = run_at_cursor(fixture);
    assert_eq!(r.has_this, expected_this, "has_this");
    assert_eq!(r.has_super, expected_super, "has_super");
}

#[rstest]
#[case::outside_any_callable("class CExample {}\n$0\n")]
#[case::after_dot_in_class_method(
    "class CExample {\n  var mField : int;\n  function Test() {\n    var local : CExample;\n    local.$0\n  }\n}\n",
)]
#[case::leading_dot_with_no_lhs("class C {\n  function A() {\n    .$0\n  }\n}\n")]
fn statement_completions_all_empty(#[case] fixture: &str) {
    let (t, r) = run_at_cursor(fixture);
    let globals = globals_for_stmt(&t, &r);
    assert!(
        r.locals.is_empty()
            && r.members.is_empty()
            && globals.is_empty()
            && !r.has_this
            && !r.has_super,
        "expected all-empty, got locals={:?} members={:?} globals={:?} has_this={} has_super={}",
        def_names(&r.locals),
        def_names(&r.members),
        def_names(&globals),
        r.has_this,
        r.has_super,
    );
}

#[test]
fn parameter_appears_with_kind_parameter() {
    let (_t, r) = run_at_cursor("function Test(owner : int) {\n  $0owner;\n}\n");
    assert!(r
        .locals
        .iter()
        .any(|d| d.symbol.name == "owner" && d.symbol.kind == SymbolKind::Parameter));
}

#[test]
fn enum_members_appear_in_globals_with_correct_kind() {
    let (t, r) = run_at_cursor("enum EColor { ERed = 0, EBlue = 1 }\nfunction F() {\n  $0\n}\n");
    let globals = globals_for_stmt(&t, &r);
    let has_member = globals
        .iter()
        .any(|d| d.symbol.name == "ERed" && d.symbol.kind == SymbolKind::EnumMember);
    assert!(
        has_member,
        "enum members must appear in statement-context globals; got {:?}",
        def_names(&globals)
    );
}

#[test]
fn script_env_globals_appear_in_statement_completions() {
    let t = TestDb::new("function Caller() {\n  $0\n}\n");
    let env = make_env("theGame", "CR4Game");
    let db = t.db().with_script_env(&env);
    let (uri, pos) = t.cursor();
    let r = statement_completions(&uri, t.doc_for(&uri), &db, pos);
    let globals = if r.needs_globals {
        merged_global_completions(&db)
    } else {
        Vec::new()
    };
    let global = globals
        .iter()
        .find(|d| d.symbol.name == "theGame")
        .expect("script env global must appear");
    assert_eq!(global.symbol.kind, SymbolKind::Variable);
    assert_eq!(global.symbol.type_annotation.as_deref(), Some("CR4Game"));
}

#[test]
fn script_env_globals_appear_in_expression_completions() {
    let t = TestDb::new("function Caller() : int {\n  return $0\n}\n");
    let env = make_env("theGame", "CR4Game");
    let db = t.db().with_script_env(&env);
    let (uri, pos) = t.cursor();
    let r = expression_completions(&uri, t.doc_for(&uri), &db, pos)
        .expect("expression completions should fire after `return `");
    assert!(r.needs_globals);
    let globals = merged_global_completions(&db);
    assert!(globals.iter().any(|d| d.symbol.name == "theGame"));
}

fn stmt_at(t: &TestDb, line: u32, character: u32) -> StatementCompletions {
    statement_completions(
        t.primary_uri(),
        t.primary_doc(),
        &t.db(),
        SourcePosition { line, character },
    )
}

#[test]
fn in_switch_true_inside_switch_body() {
    let t = TestDb::new(include_str!("../../../tests/fixtures/valid/switch_stmt.ws"));
    for (line, label) in [
        (7, "switch body level after semicolon"),
        (4, "blank line after fall-through case label"),
    ] {
        assert!(
            stmt_at(&t, line, 0).in_switch,
            "in_switch must be true at {label}"
        );
    }
}

#[test]
fn in_switch_false_inside_nested_block_within_switch() {
    let t = TestDb::new(include_str!("../../../tests/fixtures/valid/switch_stmt.ws"));
    assert!(!stmt_at(&t, 9, 0).in_switch);
}

#[test]
fn in_switch_false_in_plain_function() {
    let (_t, r) = run_at_cursor("function Test() {\n  $0\n}\n");
    assert!(!r.in_switch);
}

#[test]
fn statement_completions_fire_after_if_condition() {
    let t = TestDb::new(include_str!("../../../tests/fixtures/valid/if_stmt.ws"));
    for (line, character, label) in [
        (3, 0, "braceless if body, next-line statement"),
        (5, 0, "braced if body"),
        (7, 24, "braceless if body, same-line return"),
    ] {
        let r = stmt_at(&t, line, character);
        assert!(
            def_names(&r.locals).contains(&"x"),
            "{label}: local `x` must be visible"
        );
    }
}

#[test]
fn in_loop_true_inside_loop_bodies() {
    let t = TestDb::new(include_str!("../../../tests/fixtures/valid/loop_stmts.ws"));
    for (line, label) in [
        (3, "for body"),
        (6, "while body"),
        (9, "do-while body"),
        (13, "if nested within a for loop"),
    ] {
        assert!(
            stmt_at(&t, line, 0).in_loop,
            "in_loop must be true inside {label}"
        );
    }
}

#[test]
fn in_loop_false_in_plain_function() {
    let (_t, r) = run_at_cursor("function Test() {\n  $0\n}\n");
    assert!(!r.in_loop);
}

#[test]
fn blank_in_class_body_yields_no_completions_anywhere() {
    let t = TestDb::new(include_str!(
        "../../../tests/fixtures/valid/completion_class_body_contexts.ws"
    ));
    let pos = SourcePosition {
        line: 2,
        character: 0,
    };
    assert!(completion_members(t.primary_uri(), t.primary_doc(), &t.db(), pos).is_empty());
    assert!(type_completions(t.primary_doc(), &t.db(), pos).is_empty());
    let stmt = stmt_at(&t, 2, 0);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && !stmt.needs_globals
            && !stmt.has_this
            && !stmt.has_super
    );
}

#[test]
fn blank_in_class_method_body_yields_statement_completions_only() {
    let t = TestDb::new(include_str!(
        "../../../tests/fixtures/valid/completion_class_body_contexts.ws"
    ));
    let pos = SourcePosition {
        line: 4,
        character: 0,
    };
    assert!(completion_members(t.primary_uri(), t.primary_doc(), &t.db(), pos).is_empty());
    assert!(type_completions(t.primary_doc(), &t.db(), pos).is_empty());
    let stmt = stmt_at(&t, 4, 0);
    assert!(stmt.has_this);
    assert!(def_names(&stmt.locals).contains(&"test"));
    assert!(def_names(&stmt.members).contains(&"field"));
}

#[test]
fn function_name_in_class_body_yields_no_statement_completions() {
    let t = TestDb::new(include_str!(
        "../../../tests/fixtures/valid/completion_declaration_contexts.ws"
    ));
    let stmt = stmt_at(&t, 4, 19);
    assert!(
        stmt.locals.is_empty() && stmt.members.is_empty() && !stmt.needs_globals && !stmt.has_this
    );
}

#[test]
fn parameter_name_position_yields_no_statement_completions() {
    let t = TestDb::new(include_str!(
        "../../../tests/fixtures/valid/completion_declaration_contexts.ws"
    ));
    for (label, character) in [("first param", 16u32), ("second param after comma", 28u32)] {
        let stmt = stmt_at(&t, 6, character);
        assert!(
            stmt.locals.is_empty()
                && stmt.members.is_empty()
                && !stmt.needs_globals
                && !stmt.has_this,
            "all-empty expected at {label}"
        );
    }
}

#[test]
fn local_var_name_position_yields_no_completions_anywhere() {
    let t = TestDb::new(include_str!(
        "../../../tests/fixtures/valid/completion_declaration_contexts.ws"
    ));
    let pos = SourcePosition {
        line: 11,
        character: 8,
    };
    assert!(completion_members(t.primary_uri(), t.primary_doc(), &t.db(), pos).is_empty());
    assert!(type_completions(t.primary_doc(), &t.db(), pos).is_empty());
    let stmt = stmt_at(&t, 11, 8);
    assert!(
        stmt.locals.is_empty() && stmt.members.is_empty() && !stmt.needs_globals && !stmt.has_this
    );
}

#[rstest]
#[case::incomplete_ident_expr_in_method_body("class C { function Foo(p : int) { v$0 } }")]
#[case::var_keyword_alone_in_method_body("class C { function Foo(p : int) { va$0r } }")]
fn statement_completions_fire_in_error_state(#[case] fixture: &str) {
    let (_t, r) = run_at_cursor(fixture);
    assert!(
        r.has_this,
        "statement completions must fire in this error state"
    );
}

#[rstest]
#[case::space_after_var_keyword("class A { function N() { var $0}}")]
#[case::var_name_in_error_state("class C { function Foo(p : int) { var $0x } }")]
fn statement_completions_blocked_at_name_being_declared(#[case] fixture: &str) {
    let (_t, r) = run_at_cursor(fixture);
    assert!(
        !r.has_this && r.locals.is_empty() && r.members.is_empty() && !r.needs_globals,
        "all-empty expected when about to declare a new name"
    );
}

#[test]
fn typing_statement_keyword_skips_globals() {
    let (_t, r) = run_at_cursor("function Test() {\n  if$0\n}\n");
    assert!(r.active);
    assert!(
        !r.needs_globals,
        "keyword token should not load global catalogue"
    );
}

#[test]
fn statement_completions_members_empty_in_free_function() {
    let (_t, r) = run_at_cursor("function Test() {\n  $0\n}\n");
    assert!(
        r.members.is_empty(),
        "members bucket must be empty when cursor is in a free function"
    );
}
