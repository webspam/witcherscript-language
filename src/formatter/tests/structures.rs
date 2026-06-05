use expect_test::{expect, Expect};
use rstest::rstest;

use super::fmt;

#[rstest]
#[case::formats_if_else("function F() { if(x){a();} else{b();} }", expect![[r#"
    function F() {
        if (x) {
            a();
        }
        else {
            b();
        }
    }
"#]])]
#[case::preserves_comment_only_class_body(
    "class C extends CPlayer {\n    // A comment\n}",
    expect![[r#"
        class C extends CPlayer {
            // A comment
        }
    "#]]
)]
#[case::formats_class_with_method(
    "class C extends B { var x : int; function M() {} }",
    expect![[r#"
        class C extends B {
            var x : int;

            function M() {}
        }
    "#]]
)]
#[case::formats_enum("enum EKind { A, B = 1, C = 2 }", expect![[r#"
    enum EKind {
        A,
        B = 1,
        C = 2
    }
"#]])]
#[case::formats_empty_state("state Idle in Owner {}", expect![[r#"
    state Idle in Owner {}
"#]])]
#[case::formats_for_loop("function F() { for(i=0;i<10;i+=1){a();} }", expect![[r#"
    function F() {
        for (i = 0; i < 10; i += 1) {
            a();
        }
    }
"#]])]
#[case::formats_for_loop_with_comma_separated_init(
    "function F() { for(i=0,count=512;i<count;i+=1){} }",
    expect![[r#"
        function F() {
            for (i = 0, count = 512; i < count; i += 1) {}
        }
    "#]]
)]
#[case::normalizes_expr_whitespace(
    "function F() { var x : int = SomeObj  .Method   (  ); }",
    expect![[r#"
        function F() {
            var x : int = SomeObj.Method();
        }
    "#]]
)]
#[case::normalizes_extra_spaces_in_call("function F() { SomeFunc   (  a,   b  ); }", expect![[r#"
    function F() {
        SomeFunc(a, b);
    }
"#]])]
#[case::inline_single_stmt_if_else(
    "function F() { if (x)\n    return;\nelse\n    break; }",
    expect![[r#"
        function F() {
            if (x) return;
            else break;
        }
    "#]]
)]
fn formats_structures(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}

#[rstest]
// Operator spacing must be normalised regardless of source whitespace.
#[case::compact("function F() { for(i=0;i<count;i+=1){} }", expect![[r#"
    function F() {
        for (i = 0; i < count; i += 1) {}
    }
"#]])]
#[case::spaced("function F() { for ( i = 0 ; i < count ; i += 1 ) {} }", expect![[r#"
    function F() {
        for (i = 0; i < count; i += 1) {}
    }
"#]])]
fn spaces_around_binary_operators_in_for_header(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
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

#[rstest]
#[case::if_stmt("function F() { if (x)\n    return; }", expect![[r#"
    function F() {
        if (x) return;
    }
"#]])]
#[case::while_stmt("function F() { while (attrIndex < 0)\n    attrIndex += count; }", expect![[r#"
    function F() {
        while (attrIndex < 0) attrIndex += count;
    }
"#]])]
#[case::for_stmt("function F() { for (i = 0; i < 10; i += 1)\n    total += i; }", expect![[r#"
    function F() {
        for (i = 0; i < 10; i += 1) total += i;
    }
"#]])]
#[case::do_while(
    "function F() { do\n    attrIndex += 1;\nwhile (attrIndex < 0); }",
    expect![[r#"
        function F() {
            do attrIndex += 1; while (attrIndex < 0)
        }
    "#]]
)]
fn inline_single_stmt_body(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}
