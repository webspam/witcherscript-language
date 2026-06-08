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
