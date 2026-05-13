use crate::document::parse_document;

fn fmt(source: &str) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(doc.tree.root_node(), &doc.source, 4, false, 100, false)
}

fn fmt_compact_colon(source: &str) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(doc.tree.root_node(), &doc.source, 4, false, 100, true)
}

fn fmt_limit(source: &str, line_limit: u32) -> String {
    let doc = parse_document(source).expect("should parse");
    super::format_document(
        doc.tree.root_node(),
        &doc.source,
        4,
        false,
        line_limit,
        false,
    )
}

#[test]
fn error_recovery_formats_valid_stmts_around_invalid() {
    // var b has extra whitespace but is valid; var a is invalid (missing type annotation)
    let input = "function Test() {\n             var    b  : int;\n    var  a;\n}";
    let output = fmt(input);
    assert!(
        output.contains("var b : int;"),
        "valid stmt should be formatted, got:\n{output}"
    );
    assert!(
        output.contains("var  a;"),
        "invalid stmt should be preserved verbatim including semicolon, got:\n{output}"
    );
}

#[test]
fn compact_colon_local_var() {
    let input = "function F() { var x : int; }";
    let output = fmt_compact_colon(input);
    assert!(output.contains("var x: int;"), "got:\n{output}");
}

#[test]
fn compact_colon_member_var() {
    let input = "class C { var x : int; }";
    let output = fmt_compact_colon(input);
    assert!(output.contains("var x: int;"), "got:\n{output}");
}

#[test]
fn compact_colon_func_param() {
    let input = "function F(x : int, y : bool) {}";
    let output = fmt_compact_colon(input);
    assert!(output.contains("(x: int, y: bool)"), "got:\n{output}");
}

#[test]
fn compact_colon_return_type() {
    let input = "function F() : bool { return true; }";
    let output = fmt_compact_colon(input);
    assert!(output.contains("function F(): bool"), "got:\n{output}");
}

#[test]
fn default_colon_style_unchanged() {
    let output = fmt("function F(x : int) : bool { var y : int; return true; }");
    assert!(output.contains("(x : int)"), "got:\n{output}");
    assert!(output.contains(") : bool"), "got:\n{output}");
    assert!(output.contains("var y : int;"), "got:\n{output}");
}

#[test]
fn formats_simple_function() {
    let input = "function Foo(x:int):bool{return true;}";
    let output = fmt(input);
    assert!(output.contains("function Foo(x : int) : bool {"));
    assert!(output.contains("    return true;"));
    assert!(output.contains('}'));
}

#[test]
fn formats_if_else() {
    let input = "function F() { if(x){a();} else{b();} }";
    let output = fmt(input);
    assert!(output.contains("if (x) {"), "got:\n{output}");
    assert!(
        output.contains("}\n    else {"),
        "else should be on new line, got:\n{output}"
    );
}

#[test]
fn preserves_comment_only_class_body() {
    let input = "class C extends CPlayer {\n    // A comment\n}";
    let output = fmt(input);
    assert!(
        output.contains("// A comment"),
        "comment-only class body should not be collapsed to {{}}, got:\n{output}"
    );
}

#[test]
fn formats_class_with_method() {
    let input = "class C extends B { var x : int; function M() {} }";
    let output = fmt(input);
    assert!(output.contains("class C extends B {"));
    assert!(output.contains("    var x : int;"));
    assert!(output.contains("    function M() {}"));
}

#[test]
fn formats_enum() {
    let input = "enum EKind { A, B = 1, C = 2 }";
    let output = fmt(input);
    assert!(output.contains("enum EKind {"));
    assert!(output.contains("    A,"));
    assert!(output.contains("    B = 1,"));
}

#[test]
fn formats_empty_state() {
    let input = "state Idle in Owner {}";
    let output = fmt(input);
    assert!(output.contains("state Idle in Owner {}"));
}

#[test]
fn formats_for_loop() {
    let input = "function F() { for(i=0;i<10;i+=1){a();} }";
    let output = fmt(input);
    assert!(output.contains("for (i = 0; i < 10; i += 1) {"));
}

#[test]
fn normalizes_expr_whitespace() {
    let input = "function F() { var x : int = SomeObj  .Method   (  ); }";
    let output = fmt(input);
    assert!(
        output.contains("var x : int = SomeObj.Method();"),
        "got:\n{output}"
    );
}

#[test]
fn normalizes_extra_spaces_in_call() {
    let input = "function F() { SomeFunc   (  a,   b  ); }";
    let output = fmt(input);
    assert!(output.contains("SomeFunc(a, b);"), "got:\n{output}");
}

#[test]
fn inline_single_stmt_if() {
    let input = "function F() { if (x)\n    return; }";
    let output = fmt(input);
    assert!(
        output.contains("if (x) return;"),
        "single-stmt if body should be on same line, got:\n{output}"
    );
}

#[test]
fn inline_single_stmt_if_else() {
    let input = "function F() { if (x)\n    return;\nelse\n    break; }";
    let output = fmt(input);
    assert!(output.contains("if (x) return;"), "got:\n{output}");
    assert!(
        output.contains("    else break;"),
        "else should be on new line, got:\n{output}"
    );
}

#[test]
fn block_if_else_else_on_new_line() {
    let input = "function F() { if(x){a();} else{b();} }";
    let output = fmt(input);
    assert!(output.contains("if (x) {"), "got:\n{output}");
    assert!(
        output.contains("}\n    else {"),
        "else should be on new line, got:\n{output}"
    );
}

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
fn preserves_single_blank_line_in_body() {
    let input = "function F() {\n    a();\n\n    b();\n}";
    let output = fmt(input);
    assert!(
        output.contains("a();\n\n    b();"),
        "blank line should be preserved"
    );
}

#[test]
fn collapses_multiple_blank_lines_to_one() {
    let input = "function F() {\n    a();\n\n\n    b();\n}";
    let output = fmt(input);
    assert!(
        output.contains("a();\n\n    b();"),
        "multiple blank lines should collapse to one"
    );
    assert!(
        !output.contains("a();\n\n\n"),
        "should not have two consecutive blank lines"
    );
}

#[test]
fn idempotent_on_valid_fixture() {
    let source = include_str!("../../tests/fixtures/valid/basic_function.ws");
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatter should be idempotent");
}

#[test]
fn blank_line_between_class_fields_preserved() {
    let input = "class C extends B {\n    var a : int;\n\n    var b : int;\n}";
    let output = fmt(input);
    assert!(
        output.contains("var a : int;\n\n    var b : int;"),
        "blank line between fields should be preserved, got:\n{output}"
    );
}

#[test]
fn blank_line_at_class_start_preserved() {
    let input = "class C extends B {\n\n    var a : int;\n}";
    let output = fmt(input);
    assert!(
        output.contains("{\n\n    var a : int;"),
        "leading blank line inside class body should be preserved, got:\n{output}"
    );
}

#[test]
fn multiple_blank_lines_in_class_condensed_to_one() {
    let input = "class C extends B {\n    var a : int;\n\n\n    var b : int;\n}";
    let output = fmt(input);
    assert!(
        output.contains("var a : int;\n\n    var b : int;"),
        "multiple blank lines should collapse to one, got:\n{output}"
    );
    assert!(
        !output.contains("var a : int;\n\n\n"),
        "should not have two consecutive blank lines, got:\n{output}"
    );
}

#[test]
fn no_blank_line_between_adjacent_class_fields() {
    let input = "class C extends B {\n    var a : int;\n    var b : int;\n}";
    let output = fmt(input);
    assert!(
        !output.contains("var a : int;\n\n"),
        "adjacent fields with no blank line in source should not gain one, got:\n{output}"
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
fn trailing_comment_on_error_member_var_preserved() {
    let input = "class C {\n    var x : int // trailing comment\n}";
    let output = fmt(input);
    assert!(
        output.contains("// trailing comment"),
        "trailing comment on error member_var_decl must be preserved, got:\n{output}"
    );
    assert!(
        !output.contains("// trailing comment;"),
        "spurious semicolon must not be appended after the comment, got:\n{output}"
    );
}

#[test]
fn trailing_comment_on_member_default_val_preserved() {
    let input = "class C {\n    var x : int;\n    default x = OT_None // keep me\n    ;\n}";
    let output = fmt(input);
    assert!(
        output.contains("// keep me"),
        "trailing comment on member_default_val must be preserved, got:\n{output}"
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
fn member_default_val_with_ident_value_preserved() {
    let input = "class C extends B {\n    default isPotato = OT_None;\n}";
    let output = fmt(input);
    assert!(
        output.contains("default isPotato = OT_None;"),
        "default value that is an identifier must be preserved, got:\n{output}"
    );
}

#[test]
fn local_var_init_with_ident_value_preserved() {
    let input = "function F() { var x : EOrientationTarget = OT_None; }";
    let output = fmt(input);
    assert!(
        output.contains("var x : EOrientationTarget = OT_None;"),
        "var initializer that is an identifier must be preserved, got:\n{output}"
    );
}

#[test]
fn long_call_stmt_splits_args() {
    let src =
        "function F() { SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10); }";
    let out = fmt_limit(src, 60);
    assert!(
        out.contains("SetupEnemiesCollection(\n"),
        "long call should split, got:\n{out}"
    );
    assert!(out.contains("enemyCollectionDist,\n"), "got:\n{out}");
    assert!(out.contains("findMoveTargetDist,\n"), "got:\n{out}");
    assert!(out.contains(");\n"), "got:\n{out}");
}

#[test]
fn short_call_stmt_stays_inline() {
    let src = "function F() { Foo(a, b); }";
    let out = fmt(src);
    assert!(
        !out.contains("Foo(\n"),
        "short call should stay inline, got:\n{out}"
    );
    assert!(out.contains("Foo(a, b);"), "got:\n{out}");
}

#[test]
fn split_call_stmt_is_idempotent() {
    let src =
        "function F() { SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10); }";
    let first = fmt_limit(src, 60);
    let second = fmt_limit(&first, 60);
    assert_eq!(first, second, "split call stmt should be idempotent");
}

#[test]
fn comment_before_param_preserved() {
    let input = "function lossy(/* comment */ param1 : bool) {}";
    let output = fmt(input);
    assert!(
        output.contains("/* comment */"),
        "leading comment inside param list must be preserved, got:\n{output}"
    );
    assert!(
        output.contains("param1"),
        "param name must still be present, got:\n{output}"
    );
}

#[test]
fn comment_between_params_preserved() {
    let input = "function F(a : int, /* mid */ b : bool) {}";
    let output = fmt(input);
    assert!(
        output.contains("/* mid */"),
        "inter-param comment must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_in_enum_body_preserved() {
    let input = "enum EKind {\n    // a comment\n    A,\n    B\n}";
    let output = fmt(input);
    assert!(
        output.contains("// a comment"),
        "comment inside enum body must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_in_defaults_block_preserved() {
    let input = "class C {\n    defaults {\n        // a comment\n        x = 1;\n    }\n}";
    let output = fmt(input);
    assert!(
        output.contains("// a comment"),
        "comment inside defaults block must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_trailing_param_preserved() {
    let input = "function F(a : int /* trailing */) {}";
    let output = fmt(input);
    assert!(
        output.contains("/* trailing */"),
        "trailing comment after last param must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_between_params_comma_position() {
    let input = "function F(a : bool /*b*/,/*a*/ i : int) {}";
    let output = fmt(input);
    assert!(
        output.contains("/*b*/"),
        "first comment dropped, got:\n{output}"
    );
    assert!(
        output.contains("/*a*/"),
        "second comment dropped, got:\n{output}"
    );
    assert!(
        !output.contains("bool,"),
        "comma must not appear immediately after type (before trailing comments), got:\n{output}"
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
