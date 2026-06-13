use expect_test::expect;
use rstest::rstest;

use super::super::{Extraction, extract_variable};
use crate::formatter::FormatOptions;
use crate::test_support::{TestDb, script_env};

fn run(src: &str, needle: &str, options: FormatOptions) -> (String, Option<Extraction>) {
    let t = TestDb::new(src);
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc
        .source
        .find(needle)
        .unwrap_or_else(|| panic!("needle {needle:?} not found in fixture"));
    let result = extract_variable(uri, doc, &t.db(), start..start + needle.len(), options);
    (doc.source.clone(), result)
}

fn extraction(src: &str, needle: &str) -> Extraction {
    run(src, needle, FormatOptions::default())
        .1
        .unwrap_or_else(|| panic!("expected an extraction for needle {needle:?}"))
}

fn applied_with(src: &str, needle: &str, options: FormatOptions) -> String {
    let (source, result) = run(src, needle, options);
    let x = result.unwrap_or_else(|| panic!("expected an extraction for needle {needle:?}"));
    x.apply(&source)
}

fn applied(src: &str, needle: &str) -> String {
    applied_with(src, needle, FormatOptions::default())
}

fn refused(src: &str, needle: &str) -> bool {
    run(src, needle, FormatOptions::default()).1.is_none()
}

#[rstest]
#[case::argument_uses_parameter_name(
    "function Take(amount : int) {}\nfunction F() {\n    Take(2 + 3);\n}\n",
    "2 + 3",
    "amount"
)]
#[case::call_uses_method_name_lowercased(
    "class C {\n    function GetPos() : int { return 1; }\n}\nfunction F() {\n    var c : C;\n    var r : int;\n    r = c.GetPos();\n}\n",
    "c.GetPos()",
    "getPos"
)]
#[case::member_access_uses_member_name(
    "class C {\n    var someField : int;\n}\nfunction F() {\n    var c : C;\n    var r : int;\n    r = c.someField + 1;\n}\n",
    "c.someField",
    "someField"
)]
#[case::fallback_name(
    "function F() {\n    var r : int;\n    r = 1 + 2;\n}\n",
    "1 + 2",
    "newVar"
)]
#[case::collision_appends_suffix(
    "function F() {\n    var newVar : int;\n    var r : int;\n    r = 1 + 2;\n}\n",
    "1 + 2",
    "newVar1"
)]
#[case::enclosing_class_field_collision(
    "class C {\n    var count : int;\n    function M() {\n        var c : C;\n        var r : int;\n        r = c.count + 1;\n    }\n}\n",
    "c.count",
    "count1"
)]
#[case::inherited_field_collision(
    "class B {\n    var count : int;\n}\nclass C extends B {\n    function M() {\n        var c : C;\n        var r : int;\n        r = c.count + 1;\n    }\n}\n",
    "c.count",
    "count1"
)]
#[case::method_name_is_not_a_field_collision(
    "class C {\n    function GetPos() : int { return 1; }\n    function M() {\n        var r : int;\n        r = this.GetPos();\n    }\n}\n",
    "this.GetPos()",
    "getPos"
)]
fn names_new_variable(#[case] src: &str, #[case] needle: &str, #[case] expected: &str) {
    assert_eq!(
        extraction(src, needle).name,
        expected,
        "name for {needle:?}"
    );
}

#[test]
fn inserts_after_last_leading_var_decl() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    var b : int;\n    Use(a + b);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            var b : int;
            var x : int = a + b;
            Use(x);
        }
    "]]
    .assert_eq(&applied(src, "a + b"));
}

#[test]
fn inserts_after_open_brace_when_no_var_decls() {
    let src = "function Use(x : int) {}\nfunction F() {\n    Use(1 + 2);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var x : int = 1 + 2;
            Use(x);
        }
    "]]
    .assert_eq(&applied(src, "1 + 2"));
}

#[test]
fn inserts_before_decl_when_selection_in_its_initializer() {
    let src = "function F() {\n    var a : int = 1 + 2;\n    var b : int;\n}\n";
    expect![[r"
        function F() {
            var newVar : int = 1 + 2;
            var a : int = newVar;
            var b : int;
        }
    "]]
    .assert_eq(&applied(src, "1 + 2"));
}

#[test]
fn hoists_from_nested_block_to_callable_top() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    if (true) {\n        Use(a * 2);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            var x : int = a * 2;
            if (true) {
                Use(x);
            }
        }
    "]]
    .assert_eq(&applied(src, "a * 2"));
}

#[test]
fn works_in_event_body() {
    let src = "function Use(x : int) {}\nclass C {\n    event OnSpawned() {\n        Use(1 + 2);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        class C {
            event OnSpawned() {
                var x : int = 1 + 2;
                Use(x);
            }
        }
    "]]
    .assert_eq(&applied(src, "1 + 2"));
}

#[test]
fn trims_whitespace_around_selection() {
    let src = "function Use(x : int) {}\nfunction F() {\n    Use( 1 + 2 );\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var x : int = 1 + 2;
            Use( x );
        }
    "]]
    .assert_eq(&applied(src, " 1 + 2 "));
}

#[test]
fn extracts_value_left_of_trailing_semicolon() {
    let src = "function F() {\n    var gameStarted : bool;\n    gameStarted = true;\n}\n";
    expect![[r"
        function F() {
            var gameStarted : bool;
            var newVar : bool = true;
            gameStarted = newVar;
        }
    "]]
    .assert_eq(&applied(src, "true;"));
}

#[test]
fn trailing_semicolon_does_not_change_extraction() {
    let src = "function F() {\n    var gameStarted : bool;\n    gameStarted = true;\n}\n";
    assert_eq!(
        applied(src, "true"),
        applied(src, "true;"),
        "selecting the trailing semicolon must not change the extraction"
    );
}

const NEW_EXPR_SRC: &str = "function F(entity : CGuiObject) {\n    var rewriter : CGuiObject;\n    rewriter = new CR4HudModule in entity;\n}\n";

#[test]
fn new_expr_type_selection_expands_to_whole_construction() {
    expect![[r"
        function F(entity : CGuiObject) {
            var rewriter : CGuiObject;
            var newVar : CR4HudModule = new CR4HudModule in entity;
            rewriter = newVar;
        }
    "]]
    .assert_eq(&applied(NEW_EXPR_SRC, "CR4HudModule"));
}

#[test]
fn new_expr_keyword_selection_matches_whole_construction() {
    assert_eq!(
        applied(NEW_EXPR_SRC, "new"),
        applied(NEW_EXPR_SRC, "new CR4HudModule in entity"),
        "selecting the `new` keyword extracts the whole construction"
    );
}

#[test]
fn new_expr_lifetime_object_is_not_expanded() {
    expect![[r"
        function F(entity : CGuiObject) {
            var rewriter : CGuiObject;
            var newVar : CGuiObject = entity;
            rewriter = new CR4HudModule in newVar;
        }
    "]]
    .assert_eq(&applied(NEW_EXPR_SRC, "entity;"));
}

#[test]
fn indent_follows_tab_indented_source() {
    let src = "function Use(x : int) {}\nfunction F() {\n\tvar a : int;\n\tUse(a + 1);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
        	var a : int;
        	var x : int = a + 1;
        	Use(x);
        }
    "]]
    .assert_eq(&applied(src, "a + 1"));
}

#[test]
fn compact_colon_option_changes_spacing() {
    let src = "function Use(x : int) {}\nfunction F() {\n    Use(1 + 2);\n}\n";
    let options = FormatOptions {
        compact_colon: true,
        ..FormatOptions::default()
    };
    expect![[r"
        function Use(x : int) {}
        function F() {
            var x: int = 1 + 2;
            Use(x);
        }
    "]]
    .assert_eq(&applied_with(src, "1 + 2", options));
}

#[test]
fn name_plus_name_extracts_as_string() {
    let src = "function F() {\n    var r : string;\n    r = 'a' + 'b';\n}\n";
    expect![[r"
        function F() {
            var r : string;
            var newVar : string = 'a' + 'b';
            r = newVar;
        }
    "]]
    .assert_eq(&applied(src, "'a' + 'b'"));
}

#[test]
fn derived_name_shadowing_script_global_gets_suffix() {
    let src = "class CStats {\n    var theGame : int;\n}\nfunction F() {\n    var s : CStats;\n    var r : int;\n    r = s.theGame + 1;\n}\n";
    let t = TestDb::new(src);
    let env = script_env("theGame", "CCommonGame");
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc.source.find("s.theGame").expect("needle present");
    let result = extract_variable(
        uri,
        doc,
        &t.db().with_script_env(&env),
        start..start + "s.theGame".len(),
        FormatOptions::default(),
    )
    .expect("expected an extraction");
    assert_eq!(
        result.name, "theGame1",
        "generated name must not shadow the engine global"
    );
}

#[test]
fn returns_none_for_empty_selection() {
    let t = TestDb::new("function Use(x : int) {}\nfunction F() {\n    Use(1 + 2);\n}\n");
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc.source.find("1 + 2").expect("needle present");
    assert!(
        extract_variable(uri, doc, &t.db(), start..start, FormatOptions::default()).is_none(),
        "empty selection must not extract"
    );
}

#[rstest]
#[case::partial_expression(
    "function F() {\n    var a : int;\n    var b : int;\n    var r : int;\n    r = a + b;\n}\n",
    "a +"
)]
#[case::assignment_expression("function F() {\n    var x : int;\n    x = 5;\n}\n", "x = 5")]
#[case::ternary_expression(
    "function F() {\n    var c : bool;\n    var a : int;\n    var b : int;\n    var r : int;\n    r = c ? a : b;\n}\n",
    "c ? a : b"
)]
#[case::defaults_block_is_outside_callable(
    "class C {\n    var x : int;\n    default x = 5;\n}\n",
    "5"
)]
#[case::unresolvable_call_has_unknown_type(
    "function F() {\n    var r : int;\n    r = Mystery();\n}\n",
    "Mystery()"
)]
#[case::null_literal(
    "class C {}\nfunction F() {\n    var c : C;\n    var ok : bool;\n    ok = c == NULL;\n}\n",
    "NULL"
)]
#[case::function_reference_callee(
    "function F() {\n    var r : int;\n    r = Take(1);\n}\nfunction Take(amount : int) : int { return amount; }\n",
    "Take"
)]
#[case::whole_expression_statement(
    "class C {\n    function BlockedBefore() : bool { return true; }\n    function M() {\n        var was : C;\n        was.BlockedBefore();\n    }\n}\n",
    "was.BlockedBefore()"
)]
fn refuses_unextractable_selection(#[case] src: &str, #[case] needle: &str) {
    assert!(
        refused(src, needle),
        "selection {needle:?} must not be extractable"
    );
}

// With-init extraction emits two edits (decl + replacement); the split form adds an in-place assignment.
fn edit_count(src: &str, needle: &str) -> usize {
    extraction(src, needle).edits.len()
}

#[rstest]
#[case::local_reassigned_after_selection(
    "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    Use(a + 1);\n    a = 2;\n}\n",
    "a + 1"
)]
#[case::local_compound_assigned(
    "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    Use(a + 1);\n    a += 2;\n}\n",
    "a + 1"
)]
#[case::parameter_reassigned(
    "function Use(x : int) {}\nfunction F(p : int) {\n    Use(p + 1);\n    p = 2;\n}\n",
    "p + 1"
)]
#[case::array_element_of_local_assigned(
    "function Use(x : int) {}\nfunction F() {\n    var arr : array<int>;\n    var i : int;\n    Use(arr[0] + 1);\n    arr[i] = 5;\n}\n",
    "arr[0] + 1"
)]
fn hoists_with_init_when_write_follows_selection(#[case] src: &str, #[case] needle: &str) {
    assert_eq!(
        edit_count(src, needle),
        2,
        "a write after {needle:?} cannot change its value; hoist with an initializer"
    );
}

#[rstest]
#[case::local_written_before_selection(
    "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 2;\n    Use(a + 1);\n}\n",
    "a + 1"
)]
#[case::field_written_before_selection(
    "function Use(x : int) {}\nclass C {\n    var count : int;\n    function M() {\n        count = 5;\n        Use(count + 1);\n    }\n}\n",
    "count + 1"
)]
#[case::field_written_via_this(
    "function Use(x : int) {}\nclass C {\n    var count : int;\n    function M() {\n        this.count = 5;\n        Use(count + 1);\n    }\n}\n",
    "count + 1"
)]
fn splits_when_write_precedes_selection(#[case] src: &str, #[case] needle: &str) {
    assert_eq!(
        edit_count(src, needle),
        3,
        "a write before {needle:?} forces a split, not a with-init hoist"
    );
}

fn extracted_expr(src: &str, needle: &str) -> String {
    let (source, result) = run(src, needle, FormatOptions::default());
    let x = result.unwrap_or_else(|| panic!("expected an extraction for needle {needle:?}"));
    let splice = x
        .edits
        .iter()
        .find(|s| s.text == x.name)
        .expect("selection-replacement splice present");
    source[splice.range.clone()].to_string()
}

const OR_CHAIN: &str = "function F() {\n    var a : bool;\n    var b : bool;\n    var c : bool;\n    var r : bool;\n    r = !a || !b || !c;\n}\n";
const AND_CHAIN: &str = "function F() {\n    var a : bool;\n    var b : bool;\n    var c : bool;\n    var r : bool;\n    r = !a && !b && !c;\n}\n";

#[rstest]
#[case::first_or_expands_to_left_pair(OR_CHAIN, "a || !b", "!a || !b")]
#[case::second_or_expands_to_full_chain(OR_CHAIN, "b || !c", "!a || !b || !c")]
#[case::first_and_expands_to_left_pair(AND_CHAIN, "a && !b", "!a && !b")]
#[case::second_and_expands_to_full_chain(AND_CHAIN, "b && !c", "!a && !b && !c")]
fn touching_logical_operator_expands_to_both_operands(
    #[case] src: &str,
    #[case] needle: &str,
    #[case] expected: &str,
) {
    assert_eq!(
        extracted_expr(src, needle),
        expected,
        "selection {needle:?} should extract both operands of the touched operator"
    );
}

#[test]
fn arithmetic_operator_does_not_expand() {
    let src = "function F() {\n    var a : int;\n    var b : int;\n    var c : int;\n    var r : int;\n    r = a + b + c;\n}\n";
    assert!(
        refused(src, "a + b +"),
        "non-short-circuit operators keep the exact-selection requirement"
    );
}

const CALL_CHAIN: &str = "function F() {\n    var components : CManager;\n    var spotLights : int;\n    spotLights = components.Size();\n}\nclass CManager {\n    function Size() : int { return 1; }\n}\n";
const NESTED_CALL_CHAIN: &str = "class CComp {\n    function Size() : int { return 1; }\n}\nclass CManager {\n    function GetComponent(n : name) : CComp { var c : CComp; return c; }\n}\nfunction F() {\n    var manager : CManager;\n    var r : int;\n    r = manager.GetComponent('leftArm').Size();\n}\n";

#[rstest]
#[case::receiver_then_dot(CALL_CHAIN, "s.", "components.Size()")]
#[case::dot_then_member(CALL_CHAIN, ".S", "components.Size()")]
#[case::member_then_paren(CALL_CHAIN, "e(", "components.Size()")]
#[case::method_reference_promotes_to_call(
    "class C {\n    function GetPos() : int { return 1; }\n}\nfunction F() {\n    var c : C;\n    var r : int;\n    r = c.GetPos();\n}\n",
    "c.GetPos",
    "c.GetPos()"
)]
#[case::dot_expands_whole_left_chain(
    NESTED_CALL_CHAIN,
    ".S",
    "manager.GetComponent('leftArm').Size()"
)]
fn touching_chain_boundary_expands_to_whole_value(
    #[case] src: &str,
    #[case] needle: &str,
    #[case] expected: &str,
) {
    assert_eq!(
        extracted_expr(src, needle),
        expected,
        "selection {needle:?} should extract the whole call chain"
    );
}

const RESET_CHAIN: &str = "class C {\n    var cutPending : int;\n    function Reset() {}\n    function GetMultiplier(dt : float) : bool {\n        if (dt > 5.0) {\n            Reset();\n        }\n        else if (cutPending > 0 && dt > 3.0) {\n            Reset();\n        }\n        return cutPending > 10;\n    }\n}\n";

#[test]
fn hoists_field_read_when_no_call_can_run_before_it() {
    assert_eq!(
        edit_count(RESET_CHAIN, "cutPending > 0 && dt > 3.0"),
        2,
        "a call in a mutually exclusive branch cannot precede the else-if condition"
    );
}

#[test]
fn splits_field_read_when_an_earlier_call_could_mutate_it() {
    assert_eq!(
        edit_count(RESET_CHAIN, "cutPending > 10"),
        3,
        "an overridable call before the return may mutate the field, so keep the computation in place"
    );
}

#[test]
fn hoists_field_read_despite_call_in_an_earlier_initializer() {
    let src = "class C {\n    var count : int;\n    function Setup() : int { return 1; }\n    function M() {\n        var a : int = Setup();\n        var r : int;\n        r = count + 1;\n    }\n}\n";
    assert_eq!(
        edit_count(src, "count + 1"),
        2,
        "a call in an earlier initializer runs before the inserted decl, so it cannot mutate the read"
    );
}

#[test]
fn hoists_local_only_expression_despite_preceding_call() {
    let src = "function Mutate() {}\nfunction F() {\n    var a : int;\n    Mutate();\n    var r : int;\n    r = a + 1;\n}\n";
    assert_eq!(
        edit_count(src, "a + 1"),
        2,
        "a preceding call cannot mutate a local, so the value still hoists with an initializer"
    );
}

#[test]
fn split_keeps_assignment_below_an_earlier_write() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 2;\n    Use(a + 1);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            var x : int;
            a = 2;
            x = a + 1;
            Use(x);
        }
    "]]
    .assert_eq(&applied(src, "a + 1"));
}

#[test]
fn splits_when_a_read_is_written_inside_an_enclosing_loop() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    while (a < 10) {\n        Use(a + 1);\n        a += 1;\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            var x : int;
            while (a < 10) {
                x = a + 1;
                Use(x);
                a += 1;
            }
        }
    "]]
    .assert_eq(&applied(src, "a + 1"));
}

#[test]
fn split_declares_at_callable_top_when_field_written_first() {
    let src = "function Use(x : int) {}\nclass C {\n    var count : int;\n    function M() {\n        count = 5;\n        Use(count + 1);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        class C {
            var count : int;
            function M() {
                var x : int;
                count = 5;
                x = count + 1;
                Use(x);
            }
        }
    "]]
    .assert_eq(&applied(src, "count + 1"));
}

#[test]
fn refuses_when_split_cannot_place_assignment() {
    let src = "function Fill(out target : int) : int { return 0; }\nfunction Use(x : int, y : int) {}\nfunction F() {\n    var a : int;\n    Use(Fill(a), a + 1);\n}\n";
    assert!(
        refused(src, "a + 1"),
        "\"a + 1\" has no safe in-place assignment slot and must not be offered"
    );
}

#[test]
fn split_wraps_braceless_if_body_in_block() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 2;\n    if (true) Use(a + 1);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            var x : int;
            a = 2;
            if (true) {
                x = a + 1;
                Use(x);
            }
        }
    "]]
    .assert_eq(&applied(src, "a + 1"));
}

#[test]
fn else_if_condition_assigns_before_chain_when_preceding_reads_are_pure() {
    let src = "function F() {\n    var a : int;\n    var b : int;\n    a = 1;\n    if (a > 0) {}\n    else if (a + b > 0) {}\n    else {}\n}\n";
    expect![[r"
        function F() {
            var a : int;
            var b : int;
            var newVar : int;
            a = 1;
            newVar = a + b;
            if (a > 0) {}
            else if (newVar > 0) {}
            else {}
        }
    "]]
    .assert_eq(&applied(src, "a + b"));
}

#[test]
fn split_desugars_else_if_into_else_block_when_a_preceding_condition_can_mutate() {
    let src = "function Check() : bool { return true; }\nfunction F() {\n    var a : int;\n    var b : int;\n    a = 1;\n    if (Check()) {}\n    else if (a + b > 0) {}\n    else {}\n}\n";
    expect![[r"
        function Check() : bool { return true; }
        function F() {
            var a : int;
            var b : int;
            var newVar : int;
            a = 1;
            if (Check()) {}
            else {
                newVar = a + b;
                if (newVar > 0) {}
                else {}
            }
        }
    "]]
    .assert_eq(&applied(src, "a + b"));
}

#[test]
fn wrapped_statement_reindents_its_continuation_lines() {
    let src = "function Check() : bool { return true; }\nfunction Do() {}\nfunction F() {\n    var a : int;\n    var b : int;\n    a = 1;\n    if (Check()) {}\n    else if (a + b > 0) {\n        Do();\n    }\n}\n";
    expect![[r"
        function Check() : bool { return true; }
        function Do() {}
        function F() {
            var a : int;
            var b : int;
            var newVar : int;
            a = 1;
            if (Check()) {}
            else {
                newVar = a + b;
                if (newVar > 0) {
                    Do();
                }
            }
        }
    "]]
    .assert_eq(&applied(src, "a + b"));
}

#[test]
fn hoists_braceless_if_body_with_init_when_no_write_precedes() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    if (true) Use(a + 1);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            var x : int = a + 1;
            if (true) Use(x);
        }
    "]]
    .assert_eq(&applied(src, "a + 1"));
}

#[rstest]
#[case::writes_hit_unrelated_locals_only(
    "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    var b : int;\n    Use(a + 1);\n    b = 2;\n}\n",
    "a + 1"
)]
#[case::selection_locals_are_only_read(
    "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    Use(a + 1);\n    Use(a + 2);\n}\n",
    "a + 1"
)]
#[case::member_field_shadowing_local_name_is_assigned(
    "function Use(x : int) {}\nclass C {\n    var f : int;\n    function M() {\n        var f : int;\n        Use(f + 1);\n        this.f = 2;\n    }\n}\n",
    "f + 1"
)]
#[case::writes_hit_unrelated_field_only(
    "function Use(x : int) {}\nclass C {\n    var count : int;\n    var other : int;\n    function M() {\n        other = 5;\n        Use(count + 1);\n    }\n}\n",
    "count + 1"
)]
fn allows_extraction_despite_other_writes(#[case] src: &str, #[case] needle: &str) {
    assert!(
        run(src, needle, FormatOptions::default()).1.is_some(),
        "selection {needle:?} should still be extractable"
    );
}

#[rstest]
#[case::plain_out_arg(
    "function Fill(out target : int) {}\nfunction Use(x : int) {}\nfunction F() {\n    var a : int;\n    Use(a + 1);\n    Fill(a);\n}\n",
    "a + 1"
)]
#[case::out_arg_wrapped_in_parens(
    "function Fill(out target : int) {}\nfunction Use(x : int) {}\nfunction F() {\n    var a : int;\n    Use(a + 1);\n    Fill((a));\n}\n",
    "a + 1"
)]
#[case::out_arg_on_method_call(
    concat!(
        "//- /main.ws\n",
        "function Use(x : int) {}\n",
        "function F() {\n",
        "    var h : CHelper;\n",
        "    var a : int;\n",
        "    Use(a + 1);\n",
        "    h.Fill(a);\n",
        "}\n",
        "//- /lib.ws\n",
        "class CHelper {\n",
        "    function Fill(out target : int) {}\n",
        "}\n",
    ),
    "a + 1"
)]
fn hoists_with_init_when_out_arg_write_follows(#[case] src: &str, #[case] needle: &str) {
    assert_eq!(
        edit_count(src, needle),
        2,
        "an out-arg mutation after {needle:?} cannot change its value; hoist with an initializer"
    );
}

#[rstest]
#[case::normal_parameter(
    "function Fill(target : int) {}\nfunction Use(x : int) {}\nfunction F() {\n    var a : int;\n    Use(a + 1);\n    Fill(a);\n}\n",
    "a + 1"
)]
#[case::out_arg_targets_unrelated_local(
    "function Fill(out target : int) {}\nfunction Use(x : int) {}\nfunction F() {\n    var a : int;\n    var b : int;\n    Use(a + 1);\n    Fill(b);\n}\n",
    "a + 1"
)]
fn allows_extraction_despite_out_capable_calls(#[case] src: &str, #[case] needle: &str) {
    assert!(
        run(src, needle, FormatOptions::default()).1.is_some(),
        "selection {needle:?} should still be extractable"
    );
}
