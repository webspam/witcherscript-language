use expect_test::expect;
use rstest::rstest;

use super::super::{Extraction, extract_function};
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
    let result = extract_function(uri, doc, &t.db(), start..start + needle.len(), options);
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
    let result = extract_function(
        uri,
        doc,
        &t.db(),
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
    .assert_eq(&result.apply(&doc.source));
}

#[test]
fn compact_colon_option_applies_to_signature() {
    let src = "function F() {\n    var a : int;\n    var r : int;\n    r = a + 1;\n}\n";
    let options = FormatOptions {
        compact_colon: true,
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
    let result = extract_function(
        uri,
        doc,
        &t.db().with_script_env(&env),
        start..start + "theGame + 1".len(),
        FormatOptions::default(),
    )
    .expect("expected an extraction");
    let applied = result.apply(&doc.source);
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
#[case::void_call("function F() {\n    Act();\n}\nfunction Act() {}\n", "Act()")]
#[case::unresolved_expression_type(
    "function F() {\n    var r : int;\n    r = q + 1;\n}\n",
    "q + 1"
)]
fn refuses_unextractable_selections(#[case] src: &str, #[case] needle: &str) {
    assert!(refused(src, needle), "must refuse needle {needle:?}");
}
