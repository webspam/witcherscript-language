use expect_test::expect;

use super::{fmt, fmt_limit};

#[test]
fn long_call_stmt_splits_args() {
    let src =
        "function F() { SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10); }";
    let output = fmt_limit(src, 60);
    expect![[r"
        function F() {
            SetupEnemiesCollection(
                enemyCollectionDist,
                findMoveTargetDist,
                10
            );
        }
    "]]
    .assert_eq(&output);
    assert_eq!(
        output,
        fmt_limit(&output, 60),
        "split call stmt should be idempotent"
    );
}

#[test]
fn short_call_stmt_stays_inline() {
    expect![[r"
        function F() {
            Foo(a, b);
        }
    "]]
    .assert_eq(&fmt("function F() { Foo(a, b); }"));
}

#[test]
fn empty_call_arg_keeps_space_between_commas() {
    expect![[r#"
        function F() {
            someVar.Func(true, , "test");
        }
    "#]]
    .assert_eq(&fmt("function F() { someVar.Func(true, , \"test\"); }"));
}
