use expect_test::expect;
use rstest::rstest;

use super::{fmt, fmt_limit};

#[test]
fn labels_are_indented_under_switch() {
    let src = "function F() {\nswitch (x) {\ncase 1:\nFoo();\nbreak;\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1:
                    Foo();
                    break;
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn inline_arms_align_without_break() {
    let src = "function F() {\nswitch (attrIndex) {\ncase 0: return 'brightness';\ncase 1: return 'radius';\ncase 2: return 'attenuation';\ndefault: return 'unknown';\n}\n}\n";
    expect![[r#"
        function F() {
            switch (attrIndex) {
                case 0:   return 'brightness';
                case 1:   return 'radius';
                case 2:   return 'attenuation';
                default:  return 'unknown';
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn inline_arms_align_with_break_cell() {
    let src = "function F() {\nswitch (attrIndex) {\ncase 0: attrIndex = 'brightness'; break;\ncase 1: attrIndex = 'radius'; break;\ncase 2: attrIndex = 'attenuation'; break;\ndefault: attrIndex = 'unknown'; break;\n}\n}\n";
    expect![[r#"
        function F() {
            switch (attrIndex) {
                case 0:   attrIndex = 'brightness';   break;
                case 1:   attrIndex = 'radius';       break;
                case 2:   attrIndex = 'attenuation';  break;
                default:  attrIndex = 'unknown';      break;
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn single_non_break_statement_stays_inline() {
    let src = "function F() {\nswitch (x) {\ncase 1: Foo(); break;\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1: Foo(); break;
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn two_non_break_statements_force_block() {
    let src = "function F() {\nswitch (x) {\ncase 1: Foo(); Bar(); break;\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1:
                    Foo();
                    Bar();
                    break;
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn multi_row_source_arm_stays_block() {
    let src = "function F() {\nswitch (x) {\ncase 1:\nFoo();\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1:
                    Foo();
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn fall_through_bare_labels() {
    let src = "function F() {\nswitch (x) {\ncase 1:\ncase 2: Foo(); break;\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1:
                case 2: Foo(); break;
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn blank_line_is_preserved_but_does_not_break_run() {
    let src = "function F() {\nswitch (x) {\ncase 1: return 'a';\ncase 22: return 'b';\n\ncase 3: return 'c';\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1:   return 'a';
                case 22:  return 'b';

                case 3:   return 'c';
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn comment_inside_arm_forces_block() {
    let src = "function F() {\nswitch (x) {\ncase 1: /* note */ Foo(); break;\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1: /* note */
                    Foo();
                    break;
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn comment_between_arms_is_preserved_within_run() {
    let src =
        "function F() {\nswitch (x) {\ncase 1: return 'a';\n// sep\ncase 22: return 'b';\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1:   return 'a';
                // sep
                case 22:  return 'b';
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn line_limit_demotes_run_to_block() {
    let src = "function F() {\nswitch (x) {\ncase 1: Foo(); break;\ncase 2: Bar(); break;\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1:
                    Foo();
                    break;
                case 2:
                    Bar();
                    break;
            }
        }
    "#]]
    .assert_eq(&fmt_limit(src, 20));
}

#[test]
fn empty_switch() {
    let src = "function F() {\nswitch (x) {\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[test]
fn labels_only_no_statements() {
    let src = "function F() {\nswitch (x) {\ncase 1:\ndefault:\n}\n}\n";
    expect![[r#"
        function F() {
            switch (x) {
                case 1:
                default:
            }
        }
    "#]]
    .assert_eq(&fmt(src));
}

#[rstest]
#[case::indented("function F() {\nswitch (x) {\ncase 1:\nFoo();\nbreak;\n}\n}\n")]
#[case::inline_no_break(
    "function F() {\nswitch (x) {\ncase 0: return 'a';\ndefault: return 'b';\n}\n}\n"
)]
#[case::inline_break(
    "function F() {\nswitch (x) {\ncase 0: x = 'a'; break;\ndefault: x = 'b'; break;\n}\n}\n"
)]
#[case::block("function F() {\nswitch (x) {\ncase 1: Foo(); Bar(); break;\n}\n}\n")]
#[case::fall_through("function F() {\nswitch (x) {\ncase 1:\ncase 2: Foo(); break;\n}\n}\n")]
#[case::blank("function F() {\nswitch (x) {\ncase 1: return 'a';\n\ncase 2: return 'b';\n}\n}\n")]
#[case::comment_inside("function F() {\nswitch (x) {\ncase 1: /* n */ Foo(); break;\n}\n}\n")]
#[case::comment_between(
    "function F() {\nswitch (x) {\ncase 1: return 'a';\n// sep\ncase 2: return 'b';\n}\n}\n"
)]
fn switch_formatting_is_idempotent(#[case] src: &str) {
    let once = fmt(src);
    let twice = fmt(&once);
    assert_eq!(once, twice, "switch formatting must be idempotent");
}
