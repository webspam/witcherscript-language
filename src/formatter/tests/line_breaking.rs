use expect_test::{expect, Expect};
use rstest::rstest;

use super::{fmt, fmt_limit};

#[test]
fn long_line_forces_block_form() {
    let long_cond =
        "veryLongVariableName.IsWayTooLong.SeriouslyNeedsToBeSmaller().DoesntFitWell > 1";
    let input = format!("function F() {{\n    if (expr) DoThing();\n    else if ({long_cond})\n        return;\n    else\n        Log(\"Something\");\n}}");
    expect![[r#"
        function F() {
            if (expr) {
                DoThing();
            }
            else if (veryLongVariableName.IsWayTooLong.SeriouslyNeedsToBeSmaller().DoesntFitWell > 1) {
                return;
            }
            else {
                Log("Something");
            }
        }
    "#]].assert_eq(&fmt(&input));
}

#[rstest]
#[case::long_func_signature_splits_params(
    "function LongFuncName(paramOne:int,paramTwo:bool,paramThree:string):bool{}",
    60,
    expect![[r#"
        function LongFuncName(
            paramOne : int,
            paramTwo : bool,
            paramThree : string
        ) : bool {}
    "#]]
)]
#[case::short_func_signature_stays_inline(
    "function Short(a:int):bool{return true;}",
    100,
    expect![[r#"
        function Short(a : int) : bool {
            return true;
        }
    "#]]
)]
#[case::no_param_func_never_splits("function NoParams():bool{return true;}", 10, expect![[r#"
    function NoParams() : bool {
        return true;
    }
"#]])]
#[case::long_unsplittable_if_condition_stays_inline(
    "function F() { if (thePlayer.GetWorldPosition()) continue; }",
    20,
    expect![[r#"
        function F() {
            if (thePlayer.GetWorldPosition()) {
                continue;
            }
        }
    "#]]
)]
#[case::long_if_condition_splits_onto_own_lines(
    "function F() { if (alpha || beta || gamma) return; }",
    30,
    expect![[r#"
        function F() {
            if (
                alpha ||
                beta ||
                gamma
            ) {
                return;
            }
        }
    "#]]
)]
#[case::long_if_condition_with_and_operators(
    "function F() { if (conditionAlpha && conditionBeta && conditionGamma) return; }",
    40,
    expect![[r#"
        function F() {
            if (
                conditionAlpha &&
                conditionBeta &&
                conditionGamma
            ) {
                return;
            }
        }
    "#]]
)]
fn line_limited_breaking(#[case] input: &str, #[case] limit: u32, #[case] expected: Expect) {
    expected.assert_eq(&fmt_limit(input, limit));
}

#[test]
fn class_method_params_wrapped_when_body_has_error() {
    let input = concat!(
        "class C {\n",
        "    function SomeLongMethodName(firstParam : SomeLongType, secondParam : AnotherLongType, thirdParam : YetAnotherType) : bool {\n",
        "        SomeCall() // missing semicolon\n",
        "    }\n",
        "}"
    );
    expect![[r#"
        class C {
            function SomeLongMethodName(
                firstParam : SomeLongType,
                secondParam : AnotherLongType,
                thirdParam : YetAnotherType
            ) : bool {
                SomeCall() // missing semicolon
            }
        }
    "#]]
    .assert_eq(&fmt(input));
}

#[rstest]
#[case::short_if_condition_not_split("function F() { if (x > 0) return; }", expect![[r#"
    function F() {
        if (x > 0) return;
    }
"#]])]
#[case::preserves_authored_break_in_return_chain(
    "function F() : bool {\n    return StrFindFirst(entity.ToString(), \"candle\") != -1\n        && StrFindFirst(entity.ToString(), \"candle_holder\") == -1;\n}",
    expect![[r#"
        function F() : bool {
            return StrFindFirst(entity.ToString(), "candle") != -1 &&
                StrFindFirst(entity.ToString(), "candle_holder") == -1;
        }
    "#]]
)]
#[case::short_one_line_chain_stays_collapsed(
    "function F() : bool { return aaaa && bbbb; }",
    expect![[r#"
        function F() : bool {
            return aaaa && bbbb;
        }
    "#]]
)]
#[case::partial_author_break_collapses(
    "function F() : bool { return aaaa && bbbb\n        && cccc; }",
    expect![[r#"
        function F() : bool {
            return aaaa && bbbb && cccc;
        }
    "#]]
)]
#[case::preserves_authored_break_in_assignment(
    "function F() { x = longConditionAlpha\n        && longConditionBeta; }",
    expect![[r#"
        function F() {
            x = longConditionAlpha &&
                longConditionBeta;
        }
    "#]]
)]
#[case::preserves_authored_break_in_var_init(
    "function F() { var y : bool = condAlpha\n        && condBeta; }",
    expect![[r#"
        function F() {
            var y : bool = condAlpha &&
                condBeta;
        }
    "#]]
)]
#[case::authored_break_in_while_uses_paren_split(
    "function F() { while (condAlpha\n        && condBeta) { Foo(); } }",
    expect![[r#"
        function F() {
            while (
                condAlpha &&
                condBeta
            ) {
                Foo();
            }
        }
    "#]]
)]
#[case::authored_break_in_do_while_uses_paren_split(
    "function F() { do { Foo(); } while (condAlpha\n        && condBeta); }",
    expect![[r#"
        function F() {
            do {
                Foo();
            } while (
                condAlpha &&
                condBeta
            )
        }
    "#]]
)]
fn default_limit_line_breaking(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}

#[test]
fn split_signature_is_idempotent() {
    let src = "function LongFuncName(paramOne:int,paramTwo:bool,paramThree:string):bool{}";
    let first = fmt_limit(src, 60);
    let second = fmt_limit(&first, 60);
    assert_eq!(first, second, "split-param formatting should be idempotent");
}

#[test]
fn multiline_if_condition_is_idempotent() {
    let src = "function F() { if (alpha || beta || gamma) return; }";
    let first = fmt_limit(src, 30);
    let second = fmt_limit(&first, 30);
    assert_eq!(
        first, second,
        "multiline if condition formatting should be idempotent"
    );
}

#[test]
fn preserved_return_chain_is_idempotent() {
    let src = "function F() : bool {\n    return StrFindFirst(entity.ToString(), \"candle\") != -1\n        && StrFindFirst(entity.ToString(), \"candle_holder\") == -1;\n}";
    let first = fmt(src);
    let second = fmt(&first);
    assert_eq!(
        first, second,
        "preserved-break formatting should be idempotent"
    );
}
