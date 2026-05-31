use super::{fmt, fmt_limit};

#[test]
fn long_line_forces_block_form() {
    let long_cond =
        "veryLongVariableName.IsWayTooLong.SeriouslyNeedsToBeSmaller().DoesntFitWell > 1";
    let input = format!("function F() {{\n    if (expr) DoThing();\n    else if ({long_cond})\n        return;\n    else\n        Log(\"Something\");\n}}");
    let output = fmt(&input);
    assert!(
        output.contains("if (expr) {"),
        "should wrap short if to block when chain is long, got:\n{output}"
    );
    assert!(
        output.contains("else if (") && output.contains(") {"),
        "else-if should use block, got:\n{output}"
    );
    assert!(
        output.contains("else {"),
        "final else should use block, got:\n{output}"
    );
}

#[test]
fn long_func_signature_splits_params() {
    let src = "function LongFuncName(paramOne:int,paramTwo:bool,paramThree:string):bool{}";
    let out = fmt_limit(src, 60);
    assert!(
        out.contains("function LongFuncName(\n"),
        "opening paren should be followed by newline, got:\n{out}"
    );
    assert!(out.contains("    paramOne : int,\n"), "got:\n{out}");
    assert!(out.contains("    paramTwo : bool,\n"), "got:\n{out}");
    assert!(out.contains("    paramThree : string\n"), "got:\n{out}");
    assert!(
        out.contains(") : bool {"),
        "closing paren + return type, got:\n{out}"
    );
}

#[test]
fn short_func_signature_stays_inline() {
    let src = "function Short(a:int):bool{return true;}";
    let out = fmt_limit(src, 100);
    assert!(
        !out.contains("(\n"),
        "short signature should not split, got:\n{out}"
    );
    assert!(
        out.contains("function Short(a : int) : bool {"),
        "got:\n{out}"
    );
}

#[test]
fn no_param_func_never_splits() {
    let src = "function NoParams():bool{return true;}";
    let out = fmt_limit(src, 10);
    assert!(
        !out.contains("(\n"),
        "no-param func should never split, got:\n{out}"
    );
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
    let output = fmt(input);
    assert!(
        output.contains("(\n"),
        "long class method params must be split to multiple lines even when body has error, got:\n{output}"
    );
}

#[test]
fn split_signature_is_idempotent() {
    let src = "function LongFuncName(paramOne:int,paramTwo:bool,paramThree:string):bool{}";
    let first = fmt_limit(src, 60);
    let second = fmt_limit(&first, 60);
    assert_eq!(first, second, "split-param formatting should be idempotent");
}

#[test]
fn long_unsplittable_if_condition_stays_inline() {
    let src = "function F() { if (thePlayer.GetWorldPosition()) continue; }";
    let out = fmt_limit(src, 20);
    assert!(
        !out.contains("if (\n"),
        "unsplittable condition should stay inline, got:\n{out}"
    );
    assert!(
        out.contains("if (thePlayer.GetWorldPosition()) {\n"),
        "body should use block form when line limit exceeded, got:\n{out}"
    );
    assert!(out.contains("    continue;\n"), "got:\n{out}");
}

#[test]
fn long_if_condition_splits_onto_own_lines() {
    let src = "function F() { if (alpha || beta || gamma) return; }";
    let out = fmt_limit(src, 30);
    assert!(
        out.contains("if (\n"),
        "condition should open on its own line, got:\n{out}"
    );
    assert!(
        out.contains("alpha ||\n"),
        "each operand should be on its own line with op at end, got:\n{out}"
    );
    assert!(out.contains("beta ||\n"), "got:\n{out}");
    assert!(
        out.contains("gamma\n"),
        "last operand has no trailing op, got:\n{out}"
    );
    assert!(
        out.contains(") {\n"),
        "multiline condition must force block body, got:\n{out}"
    );
    assert!(out.contains("return;"), "body must be emitted, got:\n{out}");
}

#[test]
fn short_if_condition_not_split() {
    let src = "function F() { if (x > 0) return; }";
    let out = fmt(src);
    assert!(
        !out.contains("if (\n"),
        "short condition should stay inline, got:\n{out}"
    );
}

#[test]
fn long_if_condition_with_and_operators() {
    let src = "function F() { if (conditionAlpha && conditionBeta && conditionGamma) return; }";
    let out = fmt_limit(src, 40);
    assert!(out.contains("if (\n"), "got:\n{out}");
    assert!(out.contains("conditionAlpha &&\n"), "got:\n{out}");
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
fn preserves_authored_break_in_return_chain() {
    let src = "function F() : bool {\n    return StrFindFirst(entity.ToString(), \"candle\") != -1\n        && StrFindFirst(entity.ToString(), \"candle_holder\") == -1;\n}";
    let out = fmt(src);
    assert!(
        out.contains("\"candle\") != -1 &&\n        StrFindFirst"),
        "authored break should be preserved with trailing operator, got:\n{out}"
    );
}

#[test]
fn short_one_line_chain_stays_collapsed() {
    let src = "function F() : bool { return aaaa && bbbb; }";
    let out = fmt(src);
    assert!(
        out.contains("return aaaa && bbbb;"),
        "a chain authored on one line should not be broken, got:\n{out}"
    );
}

#[test]
fn partial_author_break_collapses() {
    let src = "function F() : bool { return aaaa && bbbb\n        && cccc; }";
    let out = fmt(src);
    assert!(
        out.contains("return aaaa && bbbb && cccc;"),
        "a partially-broken chain is not preserve-worthy and should collapse, got:\n{out}"
    );
}

#[test]
fn preserves_authored_break_in_assignment() {
    let src = "function F() { x = longConditionAlpha\n        && longConditionBeta; }";
    let out = fmt(src);
    assert!(
        out.contains("x = longConditionAlpha &&\n        longConditionBeta;"),
        "assignment rhs chain break should be preserved, got:\n{out}"
    );
}

#[test]
fn preserves_authored_break_in_var_init() {
    let src = "function F() { var y : bool = condAlpha\n        && condBeta; }";
    let out = fmt(src);
    assert!(
        out.contains("var y : bool = condAlpha &&\n        condBeta;"),
        "local var initializer chain break should be preserved, got:\n{out}"
    );
}

#[test]
fn authored_break_in_while_uses_paren_split() {
    let src = "function F() { while (condAlpha\n        && condBeta) { Foo(); } }";
    let out = fmt(src);
    assert!(
        out.contains("while (\n"),
        "while should open its own line, got:\n{out}"
    );
    assert!(out.contains("condAlpha &&\n"), "got:\n{out}");
    assert!(out.contains("condBeta\n"), "got:\n{out}");
    assert!(
        out.contains("    ) {"),
        "paren should close before block, got:\n{out}"
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
