use expect_test::{Expect, expect};
use rstest::rstest;

use super::{fmt, fmt_limit};

#[rstest]
#[case::break_after_open_paren_splits("function F() { Foo(\n a, b); }", expect![[r"
    function F() {
        Foo(
            a,
            b
        );
    }
"]])]
#[case::break_after_first_arg_splits("function F() { Foo(a,\n b); }", expect![[r"
    function F() {
        Foo(
            a,
            b
        );
    }
"]])]
#[case::break_before_first_comma_splits("function F() { Foo(a\n , b); }", expect![[r"
    function F() {
        Foo(
            a,
            b
        );
    }
"]])]
#[case::break_after_second_arg_collapses("function F() { Foo(a, b,\n c); }", expect![[r"
    function F() {
        Foo(a, b, c);
    }
"]])]
#[case::single_arg_break_after_open_paren_splits("function F() { Foo(\n a); }", expect![[r"
    function F() {
        Foo(
            a
        );
    }
"]])]
#[case::inner_call_break_splits_inner_only("function F() { Outer(Inner(\n a, b)); }", expect![[r"
    function F() {
        Outer(Inner(
            a,
            b
        ));
    }
"]])]
#[case::line_comment_in_later_arg_splits("function F() { Foo(a, b, // why\n c); }", expect![[r"
    function F() {
        Foo(
            a,
            b, // why
            c
        );
    }
"]])]
#[case::empty_args_collapse("function F() { Foo(\n); }", expect![[r"
    function F() {
        Foo();
    }
"]])]
#[case::break_before_close_paren_only_collapses("function F() { Foo(a, b\n); }", expect![[r"
    function F() {
        Foo(a, b);
    }
"]])]
#[case::return_call_break_splits("function F() : int { return Foo(\n a, b); }", expect![[r"
    function F() : int {
        return Foo(
            a,
            b
        );
    }
"]])]
#[case::assign_rhs_call_break_splits("function F() { x = Foo(\n a, b); }", expect![[r"
    function F() {
        x = Foo(
            a,
            b
        );
    }
"]])]
#[case::var_init_call_break_splits("function F() { var x : int = Foo(\n a, b); }", expect![[r"
    function F() {
        var x : int = Foo(
            a,
            b
        );
    }
"]])]
#[case::if_cond_call_break_splits("function F() { if (Foo(\n a, b)) { Bar(); } }", expect![[r"
    function F() {
        if (Foo(
            a,
            b
        )) {
            Bar();
        }
    }
"]])]
#[case::member_call_break_splits("function F() { a.b.Foo(\n x, y); }", expect![[r"
    function F() {
        a.b.Foo(
            x,
            y
        );
    }
"]])]
#[case::binary_operand_call_break_splits("function F() { x = a + Foo(\n b, c); }", expect![[r"
    function F() {
        x = a + Foo(
            b,
            c
        );
    }
"]])]
#[case::ternary_branch_call_break_splits("function F() { x = c ? Foo(\n a, b) : d; }", expect![[r"
    function F() {
        x = c ? Foo(
            a,
            b
        ) : d;
    }
"]])]
#[case::switch_arm_call_break_splits(
    "function F() { switch (x) { case 1: Foo(\n a, b); break; case 2: Bar(); break; } }",
    expect![[r"
        function F() {
            switch (x) {
                case 1:
                    Foo(
                        a,
                        b
                    );
                    break;
                case 2: Bar(); break;
            }
        }
    "]]
)]
fn call_authored_layout(#[case] input: &str, #[case] expected: Expect) {
    let output = fmt(input);
    expected.assert_eq(&output);
    assert_eq!(output, fmt(&output), "call layout should be idempotent");
}

#[rstest]
#[case::return_call_splits(
    "function F() : int { return Compute(firstArg, secondArg); }",
    expect![[r"
        function F() : int {
            return Compute(
                firstArg,
                secondArg
            );
        }
    "]]
)]
#[case::var_init_call_splits(
    "function F() { var x : int = Compute(first, second); }",
    expect![[r"
        function F() {
            var x : int = Compute(
                first,
                second
            );
        }
    "]]
)]
#[case::single_arg_call_splits(
    "function F() { Compute(someVeryLongArgument); }",
    expect![[r"
        function F() {
            Compute(
                someVeryLongArgument
            );
        }
    "]]
)]
#[case::outer_call_splits_before_inner(
    "function F() { Outer(Inner(a, b), second); }",
    expect![[r"
        function F() {
            Outer(
                Inner(a, b),
                second
            );
        }
    "]]
)]
#[case::lone_call_arg_hugs(
    "function F() { One(Two(veryLongCondHere)); }",
    expect![[r"
        function F() {
            One(Two(
                veryLongCondHere
            ));
        }
    "]]
)]
#[case::nested_lone_call_args_hug(
    "function F() { One(Two(Three(first, second))); }",
    expect![[r"
        function F() {
            One(Two(Three(
                first,
                second
            )));
        }
    "]]
)]
#[case::authored_break_before_lone_call_arg_preserved(
    "function F() { One(\n Two(veryLongCondHere)); }",
    expect![[r"
        function F() {
            One(
                Two(veryLongCondHere)
            );
        }
    "]]
)]
#[case::lone_call_arg_without_args_does_not_hug(
    "function F() { Compute(GetSomeLongValue()); }",
    expect![[r"
        function F() {
            Compute(
                GetSomeLongValue()
            );
        }
    "]]
)]
#[case::chained_call_splits_outermost(
    "function F() { x = a.Get(key).Compute(first, second); }",
    expect![[r"
        function F() {
            x = a.Get(key).Compute(
                first,
                second
            );
        }
    "]]
)]
#[case::if_condition_chain_splits_before_call(
    "function F() { if (alpha && Foo(bb, cc)) { Bar(); } }",
    expect![[r"
        function F() {
            if (
                alpha &&
                Foo(bb, cc)
            ) {
                Bar();
            }
        }
    "]]
)]
#[case::if_condition_call_splits(
    "function F() { if (IsReady(first, second)) { Foo(); } }",
    expect![[r"
        function F() {
            if (IsReady(
                first,
                second
            )) {
                Foo();
            }
        }
    "]]
)]
#[case::comma_pushes_arg_over_limit(
    "function F() { Outer(Inner(abcdefg, ijklmn), x); }",
    expect![[r"
        function F() {
            Outer(
                Inner(
                    abcdefg,
                    ijklmn
                ),
                x
            );
        }
    "]]
)]
#[case::last_arg_fits_at_limit(
    "function F() { Outer(x, Inner(abcdefg, ijklmn)); }",
    expect![[r"
        function F() {
            Outer(
                x,
                Inner(abcdefg, ijklmn)
            );
        }
    "]]
)]
#[case::trailing_operator_pushes_chain_operand_over_limit(
    "function F() : bool { return alpha &&\n Check(first, second) &&\n gamma; }",
    expect![[r"
        function F() : bool {
            return alpha &&
                Check(
                    first,
                    second
                ) &&
                gamma;
        }
    "]]
)]
#[case::chain_operand_call_fits_on_its_line(
    "function F() : bool { return alpha\n && Check(first, sec)\n && gamma; }",
    expect![[r"
        function F() : bool {
            return alpha
                && Check(first, sec)
                && gamma;
        }
    "]]
)]
#[case::ternary_conseq_call_fits_on_its_line(
    "function F() { x = c\n ? Foo(aa, bb)\n : ddddddddddddddddd; }",
    expect![[r"
        function F() {
            x = c
                ? Foo(aa, bb)
                : ddddddddddddddddd;
        }
    "]]
)]
#[case::ternary_cond_call_fits_on_its_line(
    "function F() { x = Check(a, b)\n ? yyyyyy\n : zzzzzz; }",
    expect![[r"
        function F() {
            x = Check(a, b)
                ? yyyyyy
                : zzzzzz;
        }
    "]]
)]
fn call_line_limit(#[case] input: &str, #[case] expected: Expect) {
    let output = fmt_limit(input, 30);
    expected.assert_eq(&output);
    assert_eq!(
        output,
        fmt_limit(&output, 30),
        "call layout should be idempotent"
    );
}

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
