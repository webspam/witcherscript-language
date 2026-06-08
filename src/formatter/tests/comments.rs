use std::collections::BTreeSet;

use expect_test::{Expect, expect};
use rstest::rstest;

use super::fmt;

// Matches a fixture comment id (`c0`, `cL1`) without matching code idents like `count`.
fn comment_ids_in_order(source: &str) -> Vec<String> {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = source.as_bytes();
    let mut ids = Vec::new();
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
                ids.push(source[i..j].to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    ids
}

fn comment_ids(source: &str) -> BTreeSet<String> {
    comment_ids_in_order(source).into_iter().collect()
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
fn comment_order_is_preserved_across_all_constructs() {
    let input = include_str!("../../../tests/fixtures/formatter/all_grammar_constructs.ws");
    let before = comment_ids_in_order(input);
    let after = comment_ids_in_order(&fmt(input));
    assert_eq!(
        before, after,
        "formatter reordered comments: source sequence must equal formatted sequence"
    );
}

#[test]
fn comments_around_return_type_keep_source_order() {
    let input = include_str!("../../../tests/fixtures/formatter/comments_around_return_type.ws");
    let output = fmt(input);
    expect![[r#"
        function f2(/* a*/ b /*c */ : bool) /* d */ : /* e */ bool //test
        {}
    "#]]
    .assert_eq(&output);
    assert_eq!(output, fmt(&output), "formatting must be idempotent");
}

#[rstest]
#[case::trailing_comment_on_error_member_var_preserved(
    "class C {\n    var x : int // trailing comment\n}",
    expect![[r#"
        class C {
            var x : int // trailing comment
        }
    "#]]
)]
#[case::trailing_comment_on_member_default_val_preserved(
    "class C {\n    var x : int;\n    default x = OT_None // keep me\n    ;\n}",
    expect![[r#"
        class C {
            var x : int;
            default x = OT_None // keep me
            ;
        }
    "#]]
)]
#[case::comment_before_param_preserved(
    "function lossy(/* comment */ param1 : bool) {}",
    expect![[r#"
        function lossy(/* comment */ param1 : bool) {}
    "#]]
)]
#[case::comment_between_params_preserved(
    "function F(a : int, /* mid */ b : bool) {}",
    expect![[r#"
        function F(a : int, /* mid */ b : bool) {}
    "#]]
)]
#[case::comment_in_enum_body_preserved("enum EKind {\n    // a comment\n    A,\n    B\n}", expect![[r#"
    enum EKind {
        // a comment
        A,
        B
    }
"#]])]
#[case::comment_in_defaults_block_preserved(
    "class C {\n    defaults {\n        // a comment\n        x = 1;\n    }\n}",
    expect![[r#"
        class C {
            defaults {
                // a comment
                x = 1;
            }
        }
    "#]]
)]
#[case::comment_trailing_param_preserved("function F(a : int /* trailing */) {}", expect![[r#"
    function F(a : int /* trailing */) {}
"#]])]
#[case::comment_between_params_comma_position(
    "function F(a : bool /*b*/,/*a*/ i : int) {}",
    expect![[r#"
        function F(a : bool /*b*/, /*a*/ i : int) {}
    "#]]
)]
// /*f*/ appears before the comma in source; /*g*/ appears after. The formatter
// must not collapse both onto one side of the comma.
#[case::comment_between_params_keeps_source_comma_position(
    "function lossy(   /*a*/   a   /*b*/  /*c*/  :/*d*//*e*/bool/*f*/,/*g*/i:int) {}",
    expect![[r#"
        function lossy(/*a*/ a /*b*/ /*c*/ : /*d*/ /*e*/ bool /*f*/, /*g*/ i : int) {}
    "#]]
)]
#[case::line_comment_between_statements_does_not_swallow_next(
    "function f() {\n    var y : int = a // c\n        + b;\n}",
    expect![[r#"
        function f() {
            var y : int = a // c
                + b;
        }
    "#]]
)]
#[case::trailing_line_comment_after_statement_stays_and_terminates(
    "function f() {\n    doSomething(); // explain\n    other();\n}",
    expect![[r#"
        function f() {
            doSomething(); // explain
            other();
        }
    "#]]
)]
#[case::trailing_comment_after_class_member_stays_trailing(
    "class C {\n    var x : int; // trailing\n    var y : int;\n}",
    expect![[r#"
        class C {
            var x : int; // trailing
            var y : int;
        }
    "#]]
)]
#[case::trailing_comment_after_enum_variant_keeps_comma_order(
    "enum E {\n    A, // first\n    B\n}",
    expect![[r#"
        enum E {
            A, // first
            B
        }
    "#]]
)]
#[case::trailing_comment_in_defaults_block_stays_trailing(
    "class C {\n    defaults {\n        x = 1; // trailing\n    }\n}",
    expect![[r#"
        class C {
            defaults {
                x = 1; // trailing
            }
        }
    "#]]
)]
#[case::own_line_comment_in_nested_block_is_indented(
    "function f() {\n    if (x) {\n        // note\n        a();\n    }\n}",
    expect![[r#"
        function f() {
            if (x) {
                // note
                a();
            }
        }
    "#]]
)]
fn comment_preservation(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
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
