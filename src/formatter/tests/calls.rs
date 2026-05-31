use super::{fmt, fmt_limit};

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
fn empty_call_arg_keeps_space_between_commas() {
    let src = "function F() { someVar.Func(true, , \"test\"); }";
    let out = fmt(src);
    assert!(
        out.contains("Func(true, , \"test\");"),
        "empty param should render as a single space, got:\n{out}"
    );
}

#[test]
fn split_call_stmt_is_idempotent() {
    let src =
        "function F() { SetupEnemiesCollection(enemyCollectionDist, findMoveTargetDist, 10); }";
    let first = fmt_limit(src, 60);
    let second = fmt_limit(&first, 60);
    assert_eq!(first, second, "split call stmt should be idempotent");
}
