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
fn long_assignment_call_splits_args() {
    let src = "function F() { spawnedSpotlight = theGame.CreateEntity(template, parentEntity.GetWorldPosition(), parentEntity.GetWorldRotation()); }";
    let output = fmt_limit(src, 80);
    expect![[r"
        function F() {
            spawnedSpotlight = theGame.CreateEntity(
                template,
                parentEntity.GetWorldPosition(),
                parentEntity.GetWorldRotation()
            );
        }
    "]]
    .assert_eq(&output);
    assert_eq!(
        output,
        fmt_limit(&output, 80),
        "split assignment call should be idempotent"
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

#[test]
fn wrapped_call_keeps_skipped_arg() {
    let src = "function F() { someVar.LongCall(arg1aaaaaaaaaa, arg2aaaaaaaaaa, arg3aaaaaaaaaa, , arg4aaaaaaaaaa); }";
    let output = fmt_limit(src, 40);
    expect![[r"
        function F() {
            someVar.LongCall(
                arg1aaaaaaaaaa,
                arg2aaaaaaaaaa,
                arg3aaaaaaaaaa,
                ,
                arg4aaaaaaaaaa
            );
        }
    "]]
    .assert_eq(&output);
    assert_eq!(
        output,
        fmt_limit(&output, 40),
        "skipped-arg wrap should be idempotent"
    );
}

#[test]
fn wrapped_call_does_not_duplicate_skipped_arg_comment() {
    let src = "function F() { someVar.LongCall(arg1aaaaaaaaaa, arg2aaaaaaaaaa, arg3aaaaaaaaaa, , // optional arg\narg4aaaaaaaaaa); }";
    let output = fmt_limit(src, 40);
    expect![[r"
        function F() {
            someVar.LongCall(
                arg1aaaaaaaaaa,
                arg2aaaaaaaaaa,
                arg3aaaaaaaaaa,
                , // optional arg
                arg4aaaaaaaaaa
            );
        }
    "]]
    .assert_eq(&output);
    assert_eq!(
        output,
        fmt_limit(&output, 40),
        "commented skipped-arg wrap should be idempotent"
    );
}
