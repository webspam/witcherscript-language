use super::fmt;

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
fn spaces_around_binary_operators_in_for_header() {
    // Operator spacing must be normalised regardless of source whitespace.
    let compact = "function F() { for(i=0;i<count;i+=1){} }";
    let spaced = "function F() { for ( i = 0 ; i < count ; i += 1 ) {} }";
    assert!(fmt(compact).contains("for (i = 0; i < count; i += 1) {"));
    assert!(fmt(spaced).contains("for (i = 0; i < count; i += 1) {"));
}

#[test]
fn formatter_never_drops_non_whitespace_content() {
    let input = "function SomeFunc() {\n    var i, count: int;\n    for (i = 0, count = 512; i < count; i += 1) {\n        break;\n    }\n}\n";
    let output = fmt(input);
    let strip_ws = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    assert_eq!(
        strip_ws(input),
        strip_ws(&output),
        "formatter dropped non-whitespace content;\ninput:\n{input}\noutput:\n{output}"
    );
}

#[test]
fn formats_for_loop_with_comma_separated_init() {
    let input = "function F() { for(i=0,count=512;i<count;i+=1){} }";
    let output = fmt(input);
    assert!(
        output.contains("for (i = 0, count = 512; i < count; i += 1) {"),
        "got:\n{output}"
    );
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
