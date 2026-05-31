use std::collections::BTreeSet;

use expect_test::expect;
use rstest::rstest;

use super::fmt;

// Matches a fixture comment id (`c0`, `cL1`) without matching code idents like `count`.
fn comment_ids(source: &str) -> BTreeSet<String> {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = source.as_bytes();
    let mut ids = BTreeSet::new();
    let mut i = 0;
    while i < bytes.len() {
        let starts_token = i == 0 || !is_ident(bytes[i - 1]);
        if bytes[i] == b'c' && starts_token {
            let mut j = i + 1;
            if j < bytes.len() && bytes[j] == b'L' {
                j += 1;
            }
            let digits_start = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let ends_token = j >= bytes.len() || !is_ident(bytes[j]);
            if j > digits_start && ends_token {
                ids.insert(source[i..j].to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    ids
}

#[test]
fn every_comment_survives_formatting_all_constructs() {
    let input = include_str!("../../../tests/fixtures/formatter/all_grammar_constructs.ws");
    let before = comment_ids(input);
    assert!(
        before.len() > 700,
        "fixture id extraction is broken: only found {} ids",
        before.len()
    );

    let output = fmt(input);
    let after = comment_ids(&output);

    let dropped: Vec<&String> = before.difference(&after).collect();
    assert!(
        dropped.is_empty(),
        "formatter dropped {} of {} comments: {dropped:?}",
        dropped.len(),
        before.len()
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
fn comment_between_params_keeps_source_comma_position() {
    // /*f*/ appears before the comma in source; /*g*/ appears after. The formatter
    // must not collapse both onto one side of the comma.
    let input = "function lossy(   /*a*/   a   /*b*/  /*c*/  :/*d*//*e*/bool/*f*/,/*g*/i:int) {}";
    let output = fmt(input);
    assert!(
        output.contains("/*f*/, /*g*/"),
        "comma must sit between /*f*/ and /*g*/ as in source, got:\n{output}"
    );
}

#[test]
fn line_comment_between_statements_does_not_swallow_next() {
    let input = "function f() {\n    var y : int = a // c\n        + b;\n}";
    let output = fmt(input);
    assert!(
        output.contains("// c\n"),
        "line comment must be newline-terminated, got:\n{output}"
    );
    assert!(
        output.contains("+ b"),
        "following tokens must survive the comment, got:\n{output}"
    );
    assert!(
        !output.contains("// c +"),
        "comment must not swallow the operator, got:\n{output}"
    );
}

#[test]
fn trailing_line_comment_after_statement_stays_and_terminates() {
    let input = "function f() {\n    doSomething(); // explain\n    other();\n}";
    let output = fmt(input);
    assert!(
        output.contains("doSomething(); // explain\n"),
        "trailing line comment must stay on the statement line and terminate, got:\n{output}"
    );
    assert!(
        output.contains("other();"),
        "next statement must survive, got:\n{output}"
    );
}

#[test]
fn trailing_comment_after_class_member_stays_trailing() {
    let input = "class C {\n    var x : int; // trailing\n    var y : int;\n}";
    let output = fmt(input);
    assert!(
        output.contains("var x : int; // trailing\n"),
        "trailing comment must stay on the member line, got:\n{output}"
    );
    assert!(
        output.contains("var y : int;"),
        "following member must survive, got:\n{output}"
    );
}

#[test]
fn trailing_comment_after_enum_variant_keeps_comma_order() {
    let input = "enum E {\n    A, // first\n    B\n}";
    let output = fmt(input);
    assert!(
        output.contains("A, // first"),
        "comma must precede the trailing comment, got:\n{output}"
    );
}

#[test]
fn trailing_comment_in_defaults_block_stays_trailing() {
    let input = "class C {\n    defaults {\n        x = 1; // trailing\n    }\n}";
    let output = fmt(input);
    assert!(
        output.contains("x = 1; // trailing"),
        "trailing comment in defaults block must stay trailing, got:\n{output}"
    );
}

#[test]
fn own_line_comment_in_nested_block_is_indented() {
    let input = "function f() {\n    if (x) {\n        // note\n        a();\n    }\n}";
    let output = fmt(input);
    assert!(
        output.contains("        // note\n"),
        "own-line comment must be indented to its block depth, got:\n{output}"
    );
}

#[test]
fn no_comment_duplicated_across_all_constructs() {
    let input = include_str!("../../../tests/fixtures/formatter/all_grammar_constructs.ws");
    let output = fmt(input);
    let dups: Vec<String> = comment_ids(input)
        .iter()
        .filter_map(|id| {
            let n = output.matches(&format!("/*{id}*/")).count()
                + output.matches(&format!("// {id}")).count();
            (n > 1).then(|| format!("{id}x{n}"))
        })
        .collect();
    assert!(dups.is_empty(), "comments emitted more than once: {dups:?}");
}

#[test]
fn line_comment_before_brace_moves_brace_to_own_indented_line() {
    let input =
        include_str!("../../../tests/fixtures/formatter/line_comment_before_brace_function.ws");
    let output = fmt(input);
    expect![[r#"
        function fn() // c0
        {
            if (true) // c1
            {
                return; // c2
            }
            // c3
        }
    "#]]
    .assert_eq(&output);
    assert_eq!(output, fmt(&output), "formatting must be idempotent");
}

#[rstest]
#[case::function(include_str!(
    "../../../tests/fixtures/formatter/line_comment_before_brace_function.ws"
))]
#[case::enumeration(include_str!(
    "../../../tests/fixtures/formatter/line_comment_before_brace_enum.ws"
))]
#[case::class(include_str!(
    "../../../tests/fixtures/formatter/line_comment_before_brace_class.ws"
))]
#[case::while_loop(include_str!(
    "../../../tests/fixtures/formatter/line_comment_before_brace_while.ws"
))]
fn line_comment_before_brace_never_swallows_brace(#[case] input: &str) {
    let once = fmt(input);
    assert!(
        !once.contains("// c0 {") && !once.contains("// c1 {") && !once.contains("// c {"),
        "line comment must not swallow the block brace, got:\n{once}"
    );
    assert_eq!(
        once,
        fmt(&once),
        "formatting must be idempotent, got:\n{once}"
    );
}

#[test]
fn formats_all_constructs_idempotently() {
    let input = include_str!("../../../tests/fixtures/formatter/all_grammar_constructs.ws");
    let once = fmt(input);
    let twice = fmt(&once);
    assert_eq!(once, twice, "formatting all constructs is not idempotent");
}
