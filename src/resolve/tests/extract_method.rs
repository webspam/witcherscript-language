use expect_test::expect;
use rstest::rstest;

use super::super::{Extraction, extract_method};
use crate::formatter::FormatOptions;
use crate::test_support::TestDb;

fn run(src: &str, needle: &str, options: FormatOptions) -> (String, Option<Extraction>) {
    let t = TestDb::new(src);
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc
        .source
        .find(needle)
        .unwrap_or_else(|| panic!("needle {needle:?} not found in fixture"));
    let result = extract_method(uri, doc, &t.db(), start..start + needle.len(), options);
    (doc.source.clone(), result)
}

fn extraction(src: &str, needle: &str) -> Extraction {
    run(src, needle, FormatOptions::default())
        .1
        .unwrap_or_else(|| panic!("expected an extraction for needle {needle:?}"))
}

fn applied(src: &str, needle: &str) -> String {
    let (source, result) = run(src, needle, FormatOptions::default());
    let x = result.unwrap_or_else(|| panic!("expected an extraction for needle {needle:?}"));
    x.apply(&source)
}

fn refused(src: &str, needle: &str) -> bool {
    run(src, needle, FormatOptions::default()).1.is_none()
}

#[test]
fn local_becomes_parameter_member_stays_implicit() {
    let src = "class C {\n    function M() {\n        var a : int;\n        var b : int;\n        var r : int;\n        r = a + b * 2;\n    }\n}\n";
    expect![[r"
        class C {
            function M() {
                var a : int;
                var b : int;
                var r : int;
                r = NewMethod(a, b);
            }

            private function NewMethod(a : int, b : int) : int {
                return a + b * 2;
            }
        }
    "]]
    .assert_eq(&applied(src, "a + b * 2"));
}

#[test]
fn private_field_is_kept_verbatim_not_promoted() {
    let src = "class CFoo {\n    private var secret : int;\n    function M() {\n        var r : int;\n        r = secret + 1;\n    }\n}\n";
    expect![[r"
        class CFoo {
            private var secret : int;
            function M() {
                var r : int;
                r = NewMethod();
            }

            private function NewMethod() : int {
                return secret + 1;
            }
        }
    "]]
    .assert_eq(&applied(src, "secret + 1"));
}

#[test]
fn this_and_public_member_need_no_receiver() {
    let src = "class CR4Player {\n    var health : int;\n    function Heal(x : int) : int { return x; }\n    function M() {\n        var r : int;\n        r = this.Heal(2) + health;\n    }\n}\n";
    expect![[r"
        class CR4Player {
            var health : int;
            function Heal(x : int) : int { return x; }
            function M() {
                var r : int;
                r = NewMethod();
            }

            private function NewMethod() : int {
                return this.Heal(2) + health;
            }
        }
    "]]
    .assert_eq(&applied(src, "this.Heal(2) + health"));
}

#[test]
fn private_method_call_is_kept_verbatim() {
    let src = "class CFoo {\n    private function Secret() : int { return 1; }\n    function M() {\n        var r : int;\n        r = Secret() + 1;\n    }\n}\n";
    expect![[r"
        class CFoo {
            private function Secret() : int { return 1; }
            function M() {
                var r : int;
                r = NewMethod();
            }

            private function NewMethod() : int {
                return Secret() + 1;
            }
        }
    "]]
    .assert_eq(&applied(src, "Secret() + 1"));
}

// Statement-level, not expression: a `super` expression's type is unresolved, so both extracts
// refuse it as a value; moving it verbatim only needs no type, which is the method's advantage.
#[test]
fn statement_run_with_super_and_sibling_call_moves_verbatim() {
    let src = "class B {\n    function Setup() {}\n}\nclass C extends B {\n    function M() {\n        super.Setup();\n        DoMore();\n    }\n    function DoMore() {}\n}\n";
    expect![[r"
        class B {
            function Setup() {}
        }
        class C extends B {
            function M() {
                NewMethod();
            }

            private function NewMethod() {
                super.Setup();
                DoMore();
            }

            function DoMore() {}
        }
    "]]
    .assert_eq(&applied(src, "super.Setup();\n        DoMore();"));
}

#[test]
fn state_member_extraction_is_supported() {
    let src = "class CFoo {}\nstate S in CFoo {\n    var counter : int;\n    function M() {\n        var r : int;\n        r = counter + 1;\n    }\n}\n";
    expect![[r"
        class CFoo {}
        state S in CFoo {
            var counter : int;
            function M() {
                var r : int;
                r = NewMethod();
            }

            private function NewMethod() : int {
                return counter + 1;
            }
        }
    "]]
    .assert_eq(&applied(src, "counter + 1"));
}

#[test]
fn written_private_field_stays_implicit_no_out_parameter() {
    let src = "class CFoo {\n    private var count : int;\n    function M() {\n        count = count + 1;\n    }\n}\n";
    expect![[r"
        class CFoo {
            private var count : int;
            function M() {
                NewMethod();
            }

            private function NewMethod() {
                count = count + 1;
            }
        }
    "]]
    .assert_eq(&applied(src, "count = count + 1;"));
}

#[test]
fn statement_run_returns_single_unconditional_output() {
    let src = "function Use(x : int) {}\nclass C {\n    function M() {\n        var x : int;\n        var a : int;\n        a = 2;\n        x = a + 3;\n        Use(x);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        class C {
            function M() {
                var x : int;
                var a : int;
                a = 2;
                x = NewMethod(a);
                Use(x);
            }

            private function NewMethod(a : int) : int {
                var x : int;
                x = a + 3;
                return x;
            }
        }
    "]]
    .assert_eq(&applied(src, "x = a + 3;"));
}

#[test]
fn multiple_written_locals_become_out_parameters() {
    let src = "function Use(x : int) {}\nclass C {\n    function M() {\n        var a : int;\n        var b : int;\n        a = 1;\n        b = 2;\n        Use(a + b);\n    }\n}\n";
    expect![[r"
        function Use(x : int) {}
        class C {
            function M() {
                var a : int;
                var b : int;
                NewMethod(a, b);
                Use(a + b);
            }

            private function NewMethod(out a : int, out b : int) {
                a = 1;
                b = 2;
            }
        }
    "]]
    .assert_eq(&applied(src, "a = 1;\n        b = 2;"));
}

#[test]
fn extracts_from_event_body_into_a_method() {
    let src = "class C {\n    var hp : int;\n    event OnHit() {\n        var r : int;\n        r = hp + 1;\n    }\n}\n";
    expect![[r"
        class C {
            var hp : int;
            event OnHit() {
                var r : int;
                r = NewMethod();
            }

            private function NewMethod() : int {
                return hp + 1;
            }
        }
    "]]
    .assert_eq(&applied(src, "hp + 1"));
}

#[rstest]
#[case::default_name(
    "class C {\n    function M() {\n        var r : int;\n        r = 1 + 2;\n    }\n}\n",
    "1 + 2",
    "NewMethod"
)]
#[case::member_collision(
    "class C {\n    function NewMethod() {}\n    function M() {\n        var r : int;\n        r = 1 + 2;\n    }\n}\n",
    "1 + 2",
    "NewMethod1"
)]
fn names_new_method(#[case] src: &str, #[case] needle: &str, #[case] expected: &str) {
    assert_eq!(
        extraction(src, needle).name,
        expected,
        "name for {needle:?}"
    );
}

#[rstest]
#[case::free_function("function F() {\n    var r : int;\n    r = 1 + 2;\n}\n", "1 + 2")]
#[case::wrap_method_free_function(
    "class CFoo {\n    function Bar() : int { return 1; }\n}\n@wrapMethod(CFoo) function Bar() : int {\n    return wrappedMethod() + 1;\n}\n",
    "wrappedMethod() + 1"
)]
#[case::add_method_free_function(
    "class CFoo {\n    var health : int;\n}\n@addMethod(CFoo) function M() {\n    var r : int;\n    r = health + 1;\n}\n",
    "health + 1"
)]
fn refuses_outside_an_inline_type_body(#[case] src: &str, #[case] needle: &str) {
    assert!(refused(src, needle), "must refuse needle {needle:?}");
}
