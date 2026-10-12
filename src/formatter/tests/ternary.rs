use expect_test::{Expect, expect};
use rstest::rstest;

use super::{fmt, fmt_limit};

#[rstest]
#[case::one_line_stays_inline("function F() : int { return a?b:c; }", expect![[r"
    function F() : int {
        return a ? b : c;
    }
"]])]
#[case::authored_break_preserved(
    "function F() : int {\n    return condition == true\n        ? ifTrue\n        : ifFalse;\n}",
    expect![[r"
        function F() : int {
            return condition == true
                ? ifTrue
                : ifFalse;
        }
    "]]
)]
#[case::irregular_indent_normalized(
    "function F() : int {\n    return condition\n  ? ifTrue\n            : ifFalse;\n}",
    expect![[r"
        function F() : int {
            return condition
                ? ifTrue
                : ifFalse;
        }
    "]]
)]
#[case::trailing_operators_become_leading(
    "function F() : int {\n    return condition ?\n        ifTrue :\n        ifFalse;\n}",
    expect![[r"
        function F() : int {
            return condition
                ? ifTrue
                : ifFalse;
        }
    "]]
)]
#[case::partial_break_breaks_both_branches(
    "function F() : int {\n    return condition ? ifTrue\n        : ifFalse;\n}",
    expect![[r"
        function F() : int {
            return condition
                ? ifTrue
                : ifFalse;
        }
    "]]
)]
#[case::assignment_break_preserved(
    "function F() {\n    x = condition\n        ? ifTrue\n        : ifFalse;\n}",
    expect![[r"
        function F() {
            x = condition
                ? ifTrue
                : ifFalse;
        }
    "]]
)]
#[case::var_init_break_preserved(
    "function F() {\n    var x : int = condition\n        ? ifTrue\n        : ifFalse;\n}",
    expect![[r"
        function F() {
            var x : int = condition
                ? ifTrue
                : ifFalse;
        }
    "]]
)]
#[case::nested_alt_break_indents_further(
    "function F() : int {\n    return a\n        ? b\n        : c\n            ? d\n            : e;\n}",
    expect![[r"
        function F() : int {
            return a
                ? b
                : c
                    ? d
                    : e;
        }
    "]]
)]
#[case::broken_cond_chain_preserved(
    "function F() : int {\n    return alpha\n        && beta\n        ? ifTrue\n        : ifFalse;\n}",
    expect![[r"
        function F() : int {
            return alpha
                && beta
                ? ifTrue
                : ifFalse;
        }
    "]]
)]
#[case::trailing_comment_on_cond_kept(
    "function F() : int {\n    return condition // why\n        ? ifTrue\n        : ifFalse;\n}",
    expect![[r"
        function F() : int {
            return condition // why
                ? ifTrue
                : ifFalse;
        }
    "]]
)]
#[case::own_line_comment_between_branches_kept(
    "function F() : int {\n    return condition\n        ? ifTrue\n        // fallback\n        : ifFalse;\n}",
    expect![[r"
        function F() : int {
            return condition
                ? ifTrue
                // fallback
                : ifFalse;
        }
    "]]
)]
#[case::authored_break_in_call_arg_preserved(
    "function F() {\n    Foo(condition\n        ? ifTrue\n        : ifFalse);\n}",
    expect![[r"
        function F() {
            Foo(condition
                ? ifTrue
                : ifFalse);
        }
    "]]
)]
#[case::authored_break_in_if_condition_preserved(
    "function F() {\n    if (condition\n        ? ifTrue\n        : ifFalse) {\n        Foo();\n    }\n}",
    expect![[r"
        function F() {
            if (condition
                ? ifTrue
                : ifFalse) {
                Foo();
            }
        }
    "]]
)]
#[case::authored_break_in_while_condition_preserved(
    "function F() {\n    while (condition\n        ? ifTrue\n        : ifFalse) {\n        Foo();\n    }\n}",
    expect![[r"
        function F() {
            while (condition
                ? ifTrue
                : ifFalse) {
                Foo();
            }
        }
    "]]
)]
#[case::authored_break_in_broken_and_chain_preserved(
    "function F() : int {\n    return alpha\n        && (cond\n        ? b\n        : c);\n}",
    expect![[r"
        function F() : int {
            return alpha
                && (cond
                    ? b
                    : c);
        }
    "]]
)]
#[case::authored_break_in_split_call_arg_preserved(
    "function F() {\n    SomeVeryLongFunctionName(firstArgumentWithLongName, secondArgumentWithLongName, cond\n        ? a\n        : b, thirdArgumentWithLongName);\n}",
    expect![[r"
        function F() {
            SomeVeryLongFunctionName(
                firstArgumentWithLongName,
                secondArgumentWithLongName,
                cond
                    ? a
                    : b,
                thirdArgumentWithLongName
            );
        }
    "]]
)]
#[case::chain_operand_breaks_kept_alongside_ternary(
    "function F() : bool {\n    return alpha\n        && Foo(x\n        || y, c\n        ? d\n        : e);\n}",
    expect![[r"
        function F() : bool {
            return alpha
                && Foo(x
                    || y, c
                    ? d
                    : e);
        }
    "]]
)]
#[case::chain_operand_breaks_kept_without_ternary(
    "function F() : bool {\n    return alpha\n        && Foo(x\n        || y, c ? d : e);\n}",
    expect![[r"
        function F() : bool {
            return alpha
                && Foo(x
                    || y, c ? d : e);
        }
    "]]
)]
fn ternary_layout(#[case] input: &str, #[case] expected: Expect) {
    let output = fmt(input);
    expected.assert_eq(&output);
    assert_eq!(output, fmt(&output), "ternary layout should be idempotent");
}

#[rstest]
#[case::long_return_wraps(
    "function F() : int { return conditionAlpha == true ? valueIfTrue : valueIfFalse; }",
    expect![[r"
        function F() : int {
            return conditionAlpha == true
                ? valueIfTrue
                : valueIfFalse;
        }
    "]]
)]
#[case::long_assignment_wraps(
    "function F() { result = conditionAlpha ? valueIfTrue : valueIfFalse; }",
    expect![[r"
        function F() {
            result = conditionAlpha
                ? valueIfTrue
                : valueIfFalse;
        }
    "]]
)]
#[case::long_var_init_wraps(
    "function F() { var result : int = conditionAlpha ? valueIfTrue : valueIfFalse; }",
    expect![[r"
        function F() {
            var result : int = conditionAlpha
                ? valueIfTrue
                : valueIfFalse;
        }
    "]]
)]
#[case::exact_fit_stays_inline("function F() : int { return aaaa ? bbbb : cccc; }", expect![[r"
    function F() : int {
        return aaaa ? bbbb : cccc;
    }
"]])]
#[case::long_call_arg_breaks_parens(
    "function F() { Foo(conditionAlpha ? valueIfTrue : valueIfFalse); }",
    expect![[r"
        function F() {
            Foo(
                conditionAlpha ? valueIfTrue : valueIfFalse
            );
        }
    "]]
)]
#[case::nested_ternary_in_call_arg_breaks_parens(
    "function F() { Foo(aaaa ? bbbb : cccc ? dddd : eeee); }",
    expect![[r"
        function F() {
            Foo(
                aaaa ? bbbb : cccc ? dddd : eeee
            );
        }
    "]]
)]
#[case::long_for_header_assignment_stays_inline(
    "function F() { var i : int; for (result = conditionAlpha ? valueIfTrue : valueIfFalse; i < 10; i += 1) Foo(); }",
    expect![[r"
        function F() {
            var i : int;
            for (result = conditionAlpha ? valueIfTrue : valueIfFalse; i < 10; i += 1) {
                Foo();
            }
        }
    "]]
)]
#[case::authored_break_in_split_call_arg_preserved_at_limit(
    "function F() {\n    FooBarBaz(firstArgument, cond\n        ? a\n        : b, thirdArgument);\n}",
    expect![[r"
        function F() {
            FooBarBaz(
                firstArgument,
                cond
                    ? a
                    : b,
                thirdArgument
            );
        }
    "]]
)]
fn ternary_line_limit(#[case] input: &str, #[case] expected: Expect) {
    let output = fmt_limit(input, 30);
    expected.assert_eq(&output);
    assert_eq!(
        output,
        fmt_limit(&output, 30),
        "ternary wrapping should be idempotent"
    );
}
