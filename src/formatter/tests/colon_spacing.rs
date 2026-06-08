use expect_test::{Expect, expect};
use rstest::rstest;

use super::{fmt, fmt_compact_colon};

#[rstest]
#[case::local_var("function F() { var x : int; }", expect![[r"
    function F() {
        var x: int;
    }
"]])]
#[case::member_var("class C { var x : int; }", expect![[r"
    class C {
        var x: int;
    }
"]])]
#[case::func_param("function F(x : int, y : bool) {}", expect![[r"
    function F(x: int, y: bool) {}
"]])]
#[case::return_type("function F() : bool { return true; }", expect![[r"
    function F(): bool {
        return true;
    }
"]])]
fn compact_colon(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt_compact_colon(input));
}

#[test]
fn default_colon_style_unchanged() {
    expect![[r"
        function F(x : int) : bool {
            var y : int;
            return true;
        }
    "]]
    .assert_eq(&fmt(
        "function F(x : int) : bool { var y : int; return true; }",
    ));
}
