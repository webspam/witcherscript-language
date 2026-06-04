use expect_test::expect;

use super::{fmt, fmt_limit};

#[test]
fn long_call_stmt_splits_args() {
    let src =
        "function F() { SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10); }";
    expect![[r#"
        function F() {
            SetupEnemiesCollection(
                enemyCollectionDist,
                findMoveTargetDist,
                10
            );
        }
    "#]]
    .assert_eq(&fmt_limit(src, 60));
}

#[test]
fn short_call_stmt_stays_inline() {
    expect![[r#"
        function F() {
            Foo(a, b);
        }
    "#]]
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

#[test]
fn split_call_stmt_is_idempotent() {
    let src =
        "function F() { SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10); }";
    let first = fmt_limit(src, 60);
    let second = fmt_limit(&first, 60);
    assert_eq!(first, second, "split call stmt should be idempotent");
}
