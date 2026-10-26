use expect_test::{Expect, expect};
use rstest::rstest;

use super::{fmt, fmt_limit};

const FLUENT_LINE_LIMIT: u32 = 30;

#[test]
fn long_chain_breaks_before_each_call() {
    let src = r#"function F() { m_flashModule.GetChildFlashSprite("mcOptionListModule").GetChildFlashSprite("tooltip").GetMemberFlashObject("textField").SetMemberFlashString("autoSize", "left"); }"#;
    let output = fmt(src);
    expect![[r#"
        function F() {
            m_flashModule
                .GetChildFlashSprite("mcOptionListModule")
                .GetChildFlashSprite("tooltip")
                .GetMemberFlashObject("textField")
                .SetMemberFlashString("autoSize", "left");
        }
    "#]]
    .assert_eq(&output);
    assert_eq!(output, fmt(&output), "fluent chain should be idempotent");
}

#[rstest]
#[case::fitting_chain_stays_inline("function F() { a.B().C().D(); }", expect![[r"
    function F() {
        a.B().C().D();
    }
"]])]
#[case::break_before_first_call_splits("function F() { a\n.B()\n.C(); }", expect![[r"
    function F() {
        a
            .B()
            .C();
    }
"]])]
#[case::break_after_dot_splits("function F() { a.\nB().C(); }", expect![[r"
    function F() {
        a
            .B()
            .C();
    }
"]])]
#[case::break_before_second_call_splits("function F() { a.B()\n.C().D(); }", expect![[r"
    function F() {
        a
            .B()
            .C()
            .D();
    }
"]])]
#[case::break_before_third_call_collapses("function F() { a.B().C()\n.D(); }", expect![[r"
    function F() {
        a.B().C().D();
    }
"]])]
#[case::single_call_break_collapses("function F() { a\n.B(); }", expect![[r"
    function F() {
        a.B();
    }
"]])]
#[case::property_links_join_following_call("function F() { a\n.B().c.d.E(); }", expect![[r"
    function F() {
        a
            .B()
            .c.d.E();
    }
"]])]
#[case::leading_properties_stay_on_head("function F() { this.a.b\n.C().D(); }", expect![[r"
    function F() {
        this.a.b
            .C()
            .D();
    }
"]])]
#[case::plain_call_head_stays_on_head("function F() { GetA(x)\n.B().C(); }", expect![[r"
    function F() {
        GetA(x)
            .B()
            .C();
    }
"]])]
#[case::assigned_chain_indents_from_statement("function F() { x = a\n.B()\n.C(); }", expect![[r"
    function F() {
        x = a
            .B()
            .C();
    }
"]])]
#[case::block_comment_in_args_keeps_fluent(
    "function F() { a\n.B()\n.C()\n.D(x, /* c */ y); }",
    expect![[r"
        function F() {
            a
                .B()
                .C()
                .D(x, /* c */ y);
        }
    "]]
)]
#[case::line_comment_in_args_keeps_fluent(
    "function F() { a\n.B()\n.C(x, // c\ny)\n.D(); }",
    expect![[r"
        function F() {
            a
                .B()
                .C(
                    x, // c
                    y
                )
                .D();
        }
    "]]
)]
#[case::trailing_line_comment_after_link_keeps_fluent(
    "function F() { a\n.B() // c\n.C()\n.D(); }",
    expect![[r"
        function F() {
            a
                .B() // c
                .C()
                .D();
        }
    "]]
)]
#[case::own_line_comment_between_links_keeps_fluent(
    "function F() { a\n.B()\n// c\n.C()\n.D(); }",
    expect![[r"
        function F() {
            a
                .B()
                // c
                .C()
                .D();
        }
    "]]
)]
#[case::trailing_block_comment_after_link_keeps_fluent(
    "function F() { a\n.B() /* c */\n.C()\n.D(); }",
    expect![[r"
        function F() {
            a
                .B() /* c */
                .C()
                .D();
        }
    "]]
)]
#[case::trailing_line_comment_after_head_keeps_fluent(
    "function F() { a // c\n.B()\n.C(); }",
    expect![[r"
        function F() {
            a // c
                .B()
                .C();
        }
    "]]
)]
#[case::property_tail_follows_last_group("function F() { a\n.B()\n.C().d; }", expect![[r"
    function F() {
        a
            .B()
            .C().d;
    }
"]])]
fn fluent_chain_authored_layout(#[case] input: &str, #[case] expected: Expect) {
    let output = fmt(input);
    expected.assert_eq(&output);
    assert_eq!(
        output,
        fmt(&output),
        "fluent chain layout should be idempotent"
    );
}

#[rstest]
#[case::overflowing_chain_breaks_fluently(
    "function F() { theGame.GetWorld().GetPlayer().Kill(); }",
    expect![[r"
        function F() {
            theGame
                .GetWorld()
                .GetPlayer()
                .Kill();
        }
    "]]
)]
#[case::overflowing_two_call_chain_breaks_fluently(
    "function F() { theGame.GetWorld().Spawn(first, second); }",
    expect![[r"
        function F() {
            theGame
                .GetWorld()
                .Spawn(first, second);
        }
    "]]
)]
#[case::overflowing_link_splits_its_args(
    "function F() { a.B().C().Spawn(firstArgument, secondArgument); }",
    expect![[r"
        function F() {
            a
                .B()
                .C()
                .Spawn(
                    firstArgument,
                    secondArgument
                );
        }
    "]]
)]
#[case::chain_arg_breaks_fluently_inside_split_call(
    "function F() { Outer(theGame.GetWorld().GetPlayer().Kill(), x); }",
    expect![[r"
        function F() {
            Outer(
                theGame
                    .GetWorld()
                    .GetPlayer()
                    .Kill(),
                x
            );
        }
    "]]
)]
fn fluent_chain_line_limit(#[case] input: &str, #[case] expected: Expect) {
    let output = fmt_limit(input, FLUENT_LINE_LIMIT);
    expected.assert_eq(&output);
    assert_eq!(
        output,
        fmt_limit(&output, FLUENT_LINE_LIMIT),
        "fluent chain layout should be idempotent"
    );
}
