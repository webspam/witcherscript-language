use expect_test::{Expect, expect};
use rstest::rstest;

use super::fmt;

#[rstest]
#[case::blank_lines_after_annotated_decl_capped_at_one(
    "@addField(CR4Player)\nvar foo : int;\n\n\n\nvar bar : int;",
    expect![[r"
        @addField(CR4Player)
        var foo : int;

        var bar : int;
    "]]
)]
#[case::preserves_single_blank_line_in_body("function F() {\n    a();\n\n    b();\n}", expect![[r"
    function F() {
        a();

        b();
    }
"]])]
#[case::collapses_multiple_blank_lines_to_one(
    "function F() {\n    a();\n\n\n    b();\n}",
    expect![[r"
        function F() {
            a();

            b();
        }
    "]]
)]
#[case::blank_line_between_class_fields_preserved(
    "class C extends B {\n    var a : int;\n\n    var b : int;\n}",
    expect![[r"
        class C extends B {
            var a : int;

            var b : int;
        }
    "]]
)]
#[case::blank_line_at_class_start_preserved(
    "class C extends B {\n\n    var a : int;\n}",
    expect![[r"
        class C extends B {

            var a : int;
        }
    "]]
)]
#[case::multiple_blank_lines_in_class_condensed_to_one(
    "class C extends B {\n    var a : int;\n\n\n    var b : int;\n}",
    expect![[r"
        class C extends B {
            var a : int;

            var b : int;
        }
    "]]
)]
#[case::no_blank_line_between_adjacent_class_fields(
    "class C extends B {\n    var a : int;\n    var b : int;\n}",
    expect![[r"
        class C extends B {
            var a : int;
            var b : int;
        }
    "]]
)]
#[case::no_blank_line_between_adjacent_abstract_methods(
    "abstract class C {\n    function Foo();\n    function Bar();\n}",
    expect![[r"
        abstract class C {
            function Foo();
            function Bar();
        }
    "]]
)]
#[case::blank_line_between_abstract_methods_preserved(
    "abstract class C {\n    function Foo();\n\n    function Bar();\n}",
    expect![[r"
        abstract class C {
            function Foo();

            function Bar();
        }
    "]]
)]
#[case::blank_line_forced_between_abstract_and_bodied_methods(
    "abstract class C {\n    function Foo();\n    function Bar() {}\n}",
    expect![[r"
        abstract class C {
            function Foo();

            function Bar() {}
        }
    "]]
)]
#[case::blank_line_forced_between_bodied_and_abstract_methods(
    "abstract class C {\n    function Foo() {}\n    function Bar();\n}",
    expect![[r"
        abstract class C {
            function Foo() {}

            function Bar();
        }
    "]]
)]
#[case::blank_after_comment_in_body_preserved(
    "function F() {\n    a();\n    // note\n\n    b();\n}",
    expect![[r"
        function F() {
            a();
            // note

            b();
        }
    "]]
)]
#[case::multiple_blanks_after_comment_in_body_condensed(
    "function F() {\n    a();\n    // note\n\n\n\n    b();\n}",
    expect![[r"
        function F() {
            a();
            // note

            b();
        }
    "]]
)]
#[case::blank_between_comments_in_body_preserved(
    "function F() {\n    a();\n    // first\n\n    // second\n    b();\n}",
    expect![[r"
        function F() {
            a();
            // first

            // second
            b();
        }
    "]]
)]
#[case::top_level_comment_hugging_next_decl_keeps_no_blank(
    "function f() {}\n// note\nfunction g() {}",
    expect![[r"
        function f() {}
        // note
        function g() {}
    "]]
)]
#[case::top_level_comment_blank_line_preserved(
    "function f() {}\n\n// note\nfunction g() {}",
    expect![[r"
        function f() {}

        // note
        function g() {}
    "]]
)]
#[case::no_blank_line_forced_between_adjacent_add_field_decls(
    "@addField(CR4Player) public var a : bool;\n@addField(CR4Player) public var b : LRDebug_LabelManager;",
    expect![[r"
        @addField(CR4Player) public var a : bool;
        @addField(CR4Player) public var b : LRDebug_LabelManager;
    "]]
)]
#[case::blank_line_between_add_field_decls_preserved(
    "@addField(CR4Player) public var a : bool;\n\n@addField(CR4Player) public var b : bool;",
    expect![[r"
        @addField(CR4Player) public var a : bool;

        @addField(CR4Player) public var b : bool;
    "]]
)]
fn blank_line_handling(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}

// An end-of-line comment must not change which blank lines the formatter keeps or forces.
#[rstest]
#[case::top_level_eol_keeps_forced_blank(
    "function f() {} // eol\nfunction g() {}",
    expect![[r"
        function f() {} // eol

        function g() {}
    "]]
)]
#[case::class_callable_eol_keeps_forced_blank(
    "class C {\n    function a() {} // eol\n    function b() {}\n}",
    expect![[r"
        class C {
            function a() {} // eol

            function b() {}
        }
    "]]
)]
#[case::func_block_eol_keeps_source_blank(
    "function f() {\n    a(); // eol\n\n    b();\n}",
    expect![[r"
        function f() {
            a(); // eol

            b();
        }
    "]]
)]
#[case::func_block_eol_no_spurious_blank(
    "function f() {\n    a(); // eol\n    b();\n}",
    expect![[r"
        function f() {
            a(); // eol
            b();
        }
    "]]
)]
#[case::switch_arm_eol_keeps_source_blank(
    "function f() {\n    switch (x) {\n        case 1: a(); // eol\n\n        case 2: b();\n    }\n}",
    expect![[r"
        function f() {
            switch (x) {
                case 1:  a(); // eol

                case 2:  b();
            }
        }
    "]]
)]
fn eol_comment_does_not_alter_blank_lines(#[case] input: &str, #[case] expected: Expect) {
    let output = fmt(input);
    expected.assert_eq(&output);
    assert_eq!(output, fmt(&output), "formatting must be idempotent");
}
