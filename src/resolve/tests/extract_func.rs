use expect_test::expect;
use rstest::rstest;

use super::super::{BodyModel, Extraction, extract_function};
use crate::formatter::{ColonSpacing, FormatOptions};
use crate::test_support::{TestDb, script_env};

fn run(src: &str, needle: &str, options: FormatOptions) -> (String, Option<Extraction>) {
    let t = TestDb::new(src);
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc
        .source
        .find(needle)
        .unwrap_or_else(|| panic!("needle {needle:?} not found in fixture"));
    let db = t.db();
    let result = BodyModel::enclosing(uri, doc, &db, start)
        .and_then(|model| extract_function(&model, start..start + needle.len(), options));
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
    x.plan.apply(&source)
}

fn applied(src: &str, needle: &str) -> String {
    applied_with(src, needle, FormatOptions::default())
}

fn refused(src: &str, needle: &str) -> bool {
    run(src, needle, FormatOptions::default()).1.is_none()
}

#[test]
fn extracts_expression_with_locals_as_parameters() {
    let src = "function F() {\n    var a : int;\n    var b : int;\n    var r : int;\n    r = a + b * 2;\n}\n";
    expect![[r"
        function F() {
            var a : int;
            var b : int;
            var r : int;
            r = NewFunction(a, b);
        }

        function NewFunction(a : int, b : int) : int {
            return a + b * 2;
        }
    "]]
    .assert_eq(&applied(src, "a + b * 2"));
}

#[test]
fn captures_enclosing_function_parameter() {
    let src = "function F(x : int) {\n    var r : int;\n    r = x + 1;\n}\n";
    expect![[r"
        function F(x : int) {
            var r : int;
            r = NewFunction(x);
        }

        function NewFunction(x : int) : int {
            return x + 1;
        }
    "]]
    .assert_eq(&applied(src, "x + 1"));
}

#[test]
fn global_function_reference_is_not_a_parameter() {
    let src = "function F() {\n    var r : int;\n    r = Go() + 1;\n}\nfunction Go() : int { return 1; }\n";
    expect![[r"
        function F() {
            var r : int;
            r = NewFunction();
        }

        function NewFunction() : int {
            return Go() + 1;
        }

        function Go() : int { return 1; }
    "]]
    .assert_eq(&applied(src, "Go() + 1"));
}

#[test]
fn this_and_field_route_through_receiver_parameter() {
    let src = "class CR4Player {\n    var health : int;\n    function Heal(x : int) : int { return x; }\n    function M() {\n        var r : int;\n        r = this.Heal(2) + health;\n    }\n}\n";
    expect![[r"
        class CR4Player {
            var health : int;
            function Heal(x : int) : int { return x; }
            function M() {
                var r : int;
                r = NewFunction(this);
            }
        }

        function NewFunction(r4Player : CR4Player) : int {
            return r4Player.Heal(2) + r4Player.health;
        }
    "]]
    .assert_eq(&applied(src, "this.Heal(2) + health"));
}

#[test]
fn implicit_method_call_gains_receiver_qualifier() {
    let src = "class CR4Player {\n    function Heal(x : int) : int { return x; }\n    function M() {\n        var r : int;\n        r = Heal(2) + 1;\n    }\n}\n";
    expect![[r"
        class CR4Player {
            function Heal(x : int) : int { return x; }
            function M() {
                var r : int;
                r = NewFunction(this);
            }
        }

        function NewFunction(r4Player : CR4Player) : int {
            return r4Player.Heal(2) + 1;
        }
    "]]
    .assert_eq(&applied(src, "Heal(2) + 1"));
}

#[test]
fn add_method_annotation_supplies_receiver_type() {
    let src = "class CR4Player {\n    var health : int;\n}\n@addMethod(CR4Player) function M() {\n    var r : int;\n    r = health + 1;\n}\n";
    expect![[r"
        class CR4Player {
            var health : int;
        }
        @addMethod(CR4Player) function M() {
            var r : int;
            r = NewFunction(this);
        }

        function NewFunction(r4Player : CR4Player) : int {
            return r4Player.health + 1;
        }
    "]]
    .assert_eq(&applied(src, "health + 1"));
}

#[test]
fn private_field_is_promoted_to_a_parameter_not_squashed() {
    let src = "class CFoo {\n    private var secret : int;\n    function M() {\n        var r : int;\n        r = secret + 1;\n    }\n}\n";
    expect![[r"
        class CFoo {
            private var secret : int;
            function M() {
                var r : int;
                r = NewFunction(secret);
            }
        }

        function NewFunction(secret : int) : int {
            return secret + 1;
        }
    "]]
    .assert_eq(&applied(src, "secret + 1"));
}

#[test]
fn private_field_reached_through_this_is_promoted() {
    let src = "class CLightRewriteSettings {\n    function WillBreakWhenExtracted() {}\n}\nclass CFoo {\n    private var settings : CLightRewriteSettings;\n    function M() {\n        this.settings.WillBreakWhenExtracted();\n    }\n}\n";
    expect![[r"
        class CLightRewriteSettings {
            function WillBreakWhenExtracted() {}
        }
        class CFoo {
            private var settings : CLightRewriteSettings;
            function M() {
                NewFunction(settings);
            }
        }

        function NewFunction(settings : CLightRewriteSettings) {
            settings.WillBreakWhenExtracted();
        }
    "]]
    .assert_eq(&applied(src, "this.settings.WillBreakWhenExtracted();"));
}

#[test]
fn protected_field_is_also_promoted() {
    let src = "class CFoo {\n    protected var hp : int;\n    function M() {\n        var r : int;\n        r = hp * 2;\n    }\n}\n";
    expect![[r"
        class CFoo {
            protected var hp : int;
            function M() {
                var r : int;
                r = NewFunction(hp);
            }
        }

        function NewFunction(hp : int) : int {
            return hp * 2;
        }
    "]]
    .assert_eq(&applied(src, "hp * 2"));
}

#[test]
fn written_private_field_becomes_an_out_parameter() {
    let src = "class CFoo {\n    private var count : int;\n    function M() {\n        count = count + 1;\n    }\n}\n";
    expect![[r"
        class CFoo {
            private var count : int;
            function M() {
                NewFunction(count);
            }
        }

        function NewFunction(out count : int) {
            count = count + 1;
        }
    "]]
    .assert_eq(&applied(src, "count = count + 1;"));
}

#[test]
fn public_member_uses_receiver_while_private_field_is_promoted() {
    let src = "class CFoo {\n    var health : int;\n    private var secret : int;\n    function M() {\n        var r : int;\n        r = health + secret;\n    }\n}\n";
    expect![[r"
        class CFoo {
            var health : int;
            private var secret : int;
            function M() {
                var r : int;
                r = NewFunction(this, secret);
            }
        }

        function NewFunction(foo : CFoo, secret : int) : int {
            return foo.health + secret;
        }
    "]]
    .assert_eq(&applied(src, "health + secret"));
}

#[rstest]
#[case::bare_private_method(
    "class CFoo {\n    private function Secret() : int { return 1; }\n    function M() {\n        var r : int;\n        r = Secret() + 1;\n    }\n}\n",
    "Secret() + 1"
)]
#[case::this_private_method(
    "class CFoo {\n    private function DirectMemberAccess() {}\n    function M() {\n        var someVar : int;\n        someVar = 13;\n        this.DirectMemberAccess();\n    }\n}\n",
    "someVar = 13;\n        this.DirectMemberAccess();"
)]
#[case::protected_method(
    "class CFoo {\n    protected function Guarded() : int { return 1; }\n    function M() {\n        var r : int;\n        r = Guarded() + 1;\n    }\n}\n",
    "Guarded() + 1"
)]
fn refuses_private_or_protected_method_access(#[case] src: &str, #[case] needle: &str) {
    assert!(refused(src, needle), "must refuse needle {needle:?}");
}

#[test]
fn receiver_name_collision_with_local_gets_suffix() {
    let src = "class CFoo {\n    var bar : int;\n    function M() {\n        var foo : int;\n        var r : int;\n        r = bar + foo;\n    }\n}\n";
    expect![[r"
        class CFoo {
            var bar : int;
            function M() {
                var foo : int;
                var r : int;
                r = NewFunction(this, foo);
            }
        }

        function NewFunction(foo1 : CFoo, foo : int) : int {
            return foo1.bar + foo;
        }
    "]]
    .assert_eq(&applied(src, "bar + foo"));
}

#[test]
fn local_written_through_out_argument_becomes_out_parameter() {
    let src = "function F() {\n    var ok : bool;\n    var v : int;\n    ok = Fill(v);\n}\nfunction Fill(out x : int) : bool { return true; }\n";
    expect![[r"
        function F() {
            var ok : bool;
            var v : int;
            ok = NewFunction(v);
        }

        function NewFunction(out v : int) : bool {
            return Fill(v);
        }

        function Fill(out x : int) : bool { return true; }
    "]]
    .assert_eq(&applied(src, "Fill(v)"));
}

#[test]
fn array_method_call_makes_array_an_out_parameter() {
    let src =
        "function F() {\n    var arr : array<int>;\n    var n : int;\n    n = arr.Size() + 1;\n}\n";
    let t = TestDb::new(src).with_builtins_index();
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc.source.find("arr.Size() + 1").expect("needle present");
    let db = t.db();
    let model = BodyModel::enclosing(uri, doc, &db, start).expect("cursor is in a function body");
    let result = extract_function(
        &model,
        start..start + "arr.Size() + 1".len(),
        FormatOptions::default(),
    )
    .expect("expected an extraction");
    expect![[r"
        function F() {
            var arr : array<int>;
            var n : int;
            n = NewFunction(arr);
        }

        function NewFunction(out arr : array<int>) : int {
            return arr.Size() + 1;
        }
    "]]
    .assert_eq(&result.plan.apply(&doc.source));
}

#[test]
fn compact_colon_option_applies_to_signature() {
    let src = "function F() {\n    var a : int;\n    var r : int;\n    r = a + 1;\n}\n";
    let options = FormatOptions {
        colon: ColonSpacing::Compact,
        ..FormatOptions::default()
    };
    expect![[r"
        function F() {
            var a : int;
            var r : int;
            r = NewFunction(a);
        }

        function NewFunction(a: int): int {
            return a + 1;
        }
    "]]
    .assert_eq(&applied_with(src, "a + 1", options));
}

#[test]
fn tab_option_indents_generated_body_with_tab() {
    let src = "function F() {\n    var a : int;\n    var r : int;\n    r = a + 1;\n}\n";
    let options = FormatOptions {
        use_tabs: true,
        ..FormatOptions::default()
    };
    let applied = applied_with(src, "a + 1", options);
    assert!(
        applied.contains("{\n\treturn a + 1;\n}"),
        "generated body must use a tab indent, got:\n{applied}"
    );
}

#[test]
fn script_global_is_not_captured() {
    let src = "function F() {\n    var r : int;\n    r = theGame + 1;\n}\n";
    let t = TestDb::new(src);
    let env = script_env("theGame", "int");
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc.source.find("theGame + 1").expect("needle present");
    let db = t.db().with_script_env(&env);
    let model = BodyModel::enclosing(uri, doc, &db, start).expect("cursor is in a function body");
    let result = extract_function(
        &model,
        start..start + "theGame + 1".len(),
        FormatOptions::default(),
    )
    .expect("expected an extraction");
    let applied = result.plan.apply(&doc.source);
    assert!(
        applied.contains("function NewFunction() : int {"),
        "engine global must not become a parameter, got:\n{applied}"
    );
}

#[rstest]
#[case::default_name(
    "function F() {\n    var r : int;\n    r = 1 + 2;\n}\n",
    "1 + 2",
    "NewFunction"
)]
#[case::global_collision(
    "function NewFunction() {}\nfunction F() {\n    var r : int;\n    r = 1 + 2;\n}\n",
    "1 + 2",
    "NewFunction1"
)]
#[case::local_collision(
    "function F() {\n    var NewFunction : int;\n    var r : int;\n    r = 1 + 2;\n}\n",
    "1 + 2",
    "NewFunction1"
)]
#[case::enclosing_class_method_collision(
    "class C {\n    function NewFunction() {}\n    function M() {\n        var r : int;\n        r = 1 + 2;\n    }\n}\n",
    "1 + 2",
    "NewFunction1"
)]
fn names_new_function(#[case] src: &str, #[case] needle: &str, #[case] expected: &str) {
    assert_eq!(
        extraction(src, needle).name,
        expected,
        "name for {needle:?}"
    );
}

#[test]
fn extracts_statement_run_as_void_function() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 1;\n    Use(a);\n    Use(a + 1);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            a = 1;
            NewFunction(a);
        }

        function NewFunction(a : int) {
            Use(a);
            Use(a + 1);
        }
    "]]
    .assert_eq(&applied(src, "Use(a);\n    Use(a + 1);"));
}

#[test]
fn void_call_statement_extracts_without_trailing_semicolon() {
    let src = "class CFoo {\n    function Do(n : int) {}\n}\nfunction F() {\n    var foo : CFoo;\n    var k : int;\n    foo.Do(k);\n}\n";
    expect![[r"
        class CFoo {
            function Do(n : int) {}
        }
        function F() {
            var foo : CFoo;
            var k : int;
            NewFunction(foo, k);
        }

        function NewFunction(foo : CFoo, k : int) {
            foo.Do(k);
        }
    "]]
    .assert_eq(&applied(src, "foo.Do(k)"));
}

#[test]
fn call_statement_extraction_ignores_trailing_semicolon() {
    let src = "function F() {\n    Act();\n}\nfunction Act() {}\n";
    assert_eq!(
        applied(src, "Act()"),
        applied(src, "Act();"),
        "the trailing semicolon must not change a call-statement extraction"
    );
}

const ASSIGNMENT_SRC: &str = "class C {\n    function IsEnabled() : bool { return true; }\n    function M() {\n        var pointLight : C;\n        var wasEnabled : bool;\n        wasEnabled = pointLight.IsEnabled();\n        Use(wasEnabled);\n    }\n}\nfunction Use(b : bool) {}\n";

#[test]
fn assignment_statement_extracts_without_trailing_semicolon() {
    expect![[r"
        class C {
            function IsEnabled() : bool { return true; }
            function M() {
                var pointLight : C;
                var wasEnabled : bool;
                wasEnabled = NewFunction(pointLight);
                Use(wasEnabled);
            }
        }

        function NewFunction(pointLight : C) : bool {
            var wasEnabled : bool;
            wasEnabled = pointLight.IsEnabled();
            return wasEnabled;
        }

        function Use(b : bool) {}
    "]]
    .assert_eq(&applied(
        ASSIGNMENT_SRC,
        "wasEnabled = pointLight.IsEnabled()",
    ));
}

#[test]
fn assignment_statement_extraction_ignores_trailing_semicolon() {
    assert_eq!(
        applied(ASSIGNMENT_SRC, "wasEnabled = pointLight.IsEnabled()"),
        applied(ASSIGNMENT_SRC, "wasEnabled = pointLight.IsEnabled();"),
        "the trailing semicolon must not change an assignment-statement extraction"
    );
}

#[test]
fn if_statement_extracts_as_void_function() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 5;\n    if (a > 0) {\n        Use(a);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            a = 5;
            NewFunction(a);
        }

        function NewFunction(a : int) {
            if (a > 0) {
                Use(a);
            }
        }
    "]]
    .assert_eq(&applied(src, "if (a > 0) {\n        Use(a);\n    }"));
}

#[test]
fn for_loop_with_continue_extracts_as_void_function() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var i : int;\n    for (i = 0; i < 3; i += 1) {\n        if (i == 1) {\n            continue;\n        }\n        Use(i);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var i : int;
            NewFunction(i);
        }

        function NewFunction(i : int) {
            for (i = 0; i < 3; i += 1) {
                if (i == 1) {
                    continue;
                }
                Use(i);
            }
        }
    "]]
    .assert_eq(&applied(
        src,
        "for (i = 0; i < 3; i += 1) {\n        if (i == 1) {\n            continue;\n        }\n        Use(i);\n    }",
    ));
}

#[test]
fn statements_inside_a_nested_block_extract() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    if (true) {\n        a = 1;\n        Use(a);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            if (true) {
                NewFunction(a);
            }
        }

        function NewFunction(a : int) {
            a = 1;
            Use(a);
        }
    "]]
    .assert_eq(&applied(src, "a = 1;\n        Use(a);"));
}

#[test]
fn out_parameter_and_return_value_combine() {
    let src = "function Use(x : int) {}\nclass CFoo {\n    private var count : int;\n    function M() {\n        var r : int;\n        count = count + 1;\n        r = count + 5;\n        Use(r);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        class CFoo {
            private var count : int;
            function M() {
                var r : int;
                r = NewFunction(count);
                Use(r);
            }
        }

        function NewFunction(out count : int) : int {
            var r : int;
            count = count + 1;
            r = count + 5;
            return r;
        }
    "]]
    .assert_eq(&applied(src, "count = count + 1;\n        r = count + 5;"));
}

#[test]
fn new_expr_type_selection_expands_to_whole_construction() {
    let src = "function F(entity : CGuiObject) {\n    var rewriter : CGuiObject;\n    rewriter = new CR4HudModule in entity;\n}\n";
    assert_eq!(
        applied(src, "CR4HudModule"),
        applied(src, "new CR4HudModule in entity"),
        "selecting the type expands to the whole new-expression"
    );
}

#[test]
fn expression_extraction_ignores_trailing_semicolon() {
    let src = "function F() {\n    var gameStarted : bool;\n    gameStarted = true;\n}\n";
    assert_eq!(
        applied(src, "true"),
        applied(src, "true;"),
        "selecting the trailing semicolon must not change an expression extraction"
    );
}

#[test]
fn multi_statement_run_ignores_trailing_semicolon() {
    let src =
        "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 1;\n    Use(a);\n}\n";
    assert_eq!(
        applied(src, "a = 1;\n    Use(a)"),
        applied(src, "a = 1;\n    Use(a);"),
        "omitting the final statement's semicolon must not change the extraction"
    );
}

#[test]
fn crlf_source_extracts_expression() {
    let src = "function F() {\r\n    var a : int;\r\n    var r : int;\r\n    r = a + 1;\r\n}\r\n";
    let out = applied(src, "a + 1");
    assert!(
        out.contains("r = NewFunction(a);"),
        "call replaces the expression, got:\n{out}"
    );
    assert!(
        out.contains("function NewFunction(a : int) : int {"),
        "generated signature present, got:\n{out}"
    );
    assert!(
        out.contains("return a + 1;"),
        "generated body present, got:\n{out}"
    );
}

#[test]
fn tab_indented_source_statement_run_uses_tab_body() {
    let src = "function Use(x : int) {}\nfunction F() {\n\tvar a : int;\n\ta = 1;\n\tUse(a);\n}\n";
    let options = FormatOptions {
        use_tabs: true,
        ..FormatOptions::default()
    };
    let out = applied_with(src, "a = 1;\n\tUse(a);", options);
    assert!(
        out.contains("function NewFunction(a : int) {\n\ta = 1;\n\tUse(a);\n}"),
        "generated body must use tab indentation, got:\n{out}"
    );
}

#[test]
fn single_unconditional_output_is_returned() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var x : int;\n    var a : int;\n    a = 2;\n    x = a + 3;\n    Use(x);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var x : int;
            var a : int;
            a = 2;
            x = NewFunction(a);
            Use(x);
        }

        function NewFunction(a : int) : int {
            var x : int;
            x = a + 3;
            return x;
        }
    "]]
    .assert_eq(&applied(src, "x = a + 3;"));
}

#[test]
fn output_reading_its_entry_value_becomes_out_parameter() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var x : int;\n    x = 1;\n    x = x + 2;\n    Use(x);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var x : int;
            x = 1;
            NewFunction(x);
            Use(x);
        }

        function NewFunction(out x : int) {
            x = x + 2;
        }
    "]]
    .assert_eq(&applied(src, "x = x + 2;"));
}

#[test]
fn multiple_outputs_all_become_out_parameters() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    var b : int;\n    a = 1;\n    b = 2;\n    Use(a + b);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            var b : int;
            NewFunction(a, b);
            Use(a + b);
        }

        function NewFunction(out a : int, out b : int) {
            a = 1;
            b = 2;
        }
    "]]
    .assert_eq(&applied(src, "a = 1;\n    b = 2;"));
}

#[test]
fn internal_local_moves_with_its_declaration() {
    let src =
        "function Use(x : int) {}\nfunction F() {\n    var t : int;\n    t = 1;\n    Use(t);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            NewFunction();
        }

        function NewFunction() {
            var t : int;
            t = 1;
            Use(t);
        }
    "]]
    .assert_eq(&applied(src, "var t : int;\n    t = 1;\n    Use(t);"));
}

#[test]
fn loop_back_edge_keeps_written_local_live() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    var n : int;\n    n = 3;\n    while (n > 0) {\n        Use(a);\n        a = 7;\n        n = n - 1;\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            var n : int;
            n = 3;
            while (n > 0) {
                Use(a);
                a = NewFunction();
                n = n - 1;
            }
        }

        function NewFunction() : int {
            var a : int;
            a = 7;
            return a;
        }
    "]]
    .assert_eq(&applied(src, "a = 7;"));
}

#[test]
fn moved_loop_keeps_relative_indentation() {
    let src = "function F() {\n    var n : int;\n    n = 3;\n    while (n > 0) {\n        n = n - 1;\n        if (n == 1) {\n            break;\n        }\n    }\n}\n";
    expect![[r"
        function F() {
            var n : int;
            n = 3;
            NewFunction(n);
        }

        function NewFunction(n : int) {
            while (n > 0) {
                n = n - 1;
                if (n == 1) {
                    break;
                }
            }
        }
    "]]
    .assert_eq(&applied(
        src,
        "while (n > 0) {\n        n = n - 1;\n        if (n == 1) {\n            break;\n        }\n    }",
    ));
}

#[test]
fn statement_run_in_method_dedents_to_global_depth() {
    let src = "class C {\n    function M() {\n        var a : int;\n        a = 1;\n        a = a + 1;\n    }\n}\n";
    expect![[r"
        class C {
            function M() {
                var a : int;
                NewFunction(a);
            }
        }

        function NewFunction(a : int) {
            a = 1;
            a = a + 1;
        }
    "]]
    .assert_eq(&applied(src, "a = 1;\n        a = a + 1;"));
}

#[test]
fn field_write_routes_through_receiver_reference() {
    let src = "class CR4Player {\n    var health : int;\n    function M() {\n        var dmg : int;\n        dmg = 5;\n        health = health - dmg;\n    }\n}\n";
    expect![[r"
        class CR4Player {
            var health : int;
            function M() {
                var dmg : int;
                dmg = 5;
                NewFunction(this, dmg);
            }
        }

        function NewFunction(r4Player : CR4Player, dmg : int) {
            r4Player.health = r4Player.health - dmg;
        }
    "]]
    .assert_eq(&applied(src, "health = health - dmg;"));
}

#[test]
fn comment_between_statements_moves_with_them() {
    let src = "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 1; // set up\n    Use(a);\n}\n";
    expect![[r"
        function Use(x : int) {}
        function F() {
            var a : int;
            NewFunction(a);
        }

        function NewFunction(a : int) {
            a = 1; // set up
            Use(a);
        }
    "]]
    .assert_eq(&applied(src, "a = 1; // set up\n    Use(a);"));
}

#[rstest]
#[case::return_inside(
    "function F() {\n    var a : int;\n    a = 1;\n    return;\n}\n",
    "a = 1;\n    return;"
)]
#[case::bare_break(
    "function F() {\n    var n : int;\n    n = 3;\n    while (n > 0) {\n        n = n - 1;\n        if (n == 1) {\n            break;\n        }\n    }\n}\n",
    "if (n == 1) {\n            break;\n        }"
)]
#[case::bare_continue(
    "function F() {\n    var n : int;\n    n = 3;\n    while (n > 0) {\n        n = n - 1;\n        continue;\n    }\n}\n",
    "n = n - 1;\n        continue;"
)]
#[case::internal_local_read_after_selection(
    "function Use(x : int) {}\nfunction F() {\n    var t : int;\n    t = 1;\n    Use(t);\n}\n",
    "var t : int;\n    t = 1;"
)]
#[case::partial_statement(
    "function Use(x : int) {}\nfunction F() {\n    var a : int;\n    a = 1;\n    Use(a);\n}\n",
    "1;\n    Use(a)"
)]
fn refuses_unextractable_statement_runs(#[case] src: &str, #[case] needle: &str) {
    assert!(refused(src, needle), "must refuse needle {needle:?}");
}

#[rstest]
#[case::comment_only("function F() {\n    // nothing here\n}\n", "// nothing here")]
#[case::whitespace_only("function F() {\n    Act();\n}\nfunction Act() {}\n", "    ")]
fn refuses_empty_selections(#[case] src: &str, #[case] needle: &str) {
    assert!(refused(src, needle), "must refuse needle {needle:?}");
}

#[test]
fn break_inside_selected_switch_is_allowed() {
    let src = "function Use(x : int) {}\nfunction F(k : int) {\n    switch (k) {\n        case 1:\n            Use(1);\n            break;\n    }\n}\n";
    let needle = "switch (k) {\n        case 1:\n            Use(1);\n            break;\n    }";
    expect![[r"
        function Use(x : int) {}
        function F(k : int) {
            NewFunction(k);
        }

        function NewFunction(k : int) {
            switch (k) {
                case 1:
                    Use(1);
                    break;
            }
        }
    "]]
    .assert_eq(&applied(src, needle));
}

#[rstest]
#[case::super_call(
    "class B {\n    function M() : int { return 1; }\n}\nclass C extends B {\n    function M() : int {\n        var r : int;\n        r = super.M() + 1;\n        return r;\n    }\n}\n",
    "super.M() + 1"
)]
#[case::state_field_receiver(
    "class CFoo {}\nstate S in CFoo {\n    var counter : int;\n    function M() {\n        var r : int;\n        r = counter + 1;\n    }\n}\n",
    "counter + 1"
)]
#[case::struct_field_receiver(
    "struct SPos {\n    var x : int;\n}\n@addMethod(SPos) function M() {\n    var r : int;\n    r = x + 1;\n}\n",
    "x + 1"
)]
#[case::wrapped_method_macro(
    "class CFoo {\n    function Bar() : int { return 1; }\n}\n@wrapMethod(CFoo) function Bar() : int {\n    return wrappedMethod() + 1;\n}\n",
    "wrappedMethod() + 1"
)]
#[case::bare_callee_selection(
    "function F() {\n    var r : int;\n    r = Go() + 1;\n}\nfunction Go() : int { return 1; }\n",
    "Go"
)]
#[case::unresolved_expression_type(
    "function F() {\n    var r : int;\n    r = q + 1;\n}\n",
    "q + 1"
)]
fn refuses_unextractable_selections(#[case] src: &str, #[case] needle: &str) {
    assert!(refused(src, needle), "must refuse needle {needle:?}");
}
