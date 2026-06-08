use expect_test::expect;

use super::{AnnotationPlacement, fmt, fmt_aligned, fmt_aligned_with_default_placement};

fn default_keyword_col(line: &str, field: &str) -> usize {
    line.find(&format!("default {field}"))
        .unwrap_or_else(|| panic!("line missing `default {field}`:\n{line}"))
}

#[test]
fn member_colons_not_aligned_by_default() {
    expect![[r"
        class C {
            var x : int;
            var someLongName : string;
        }
    "]]
    .assert_eq(&fmt("class C { var x : int; var someLongName : string; }"));
}

#[test]
fn aligns_consecutive_member_colons() {
    expect![[r"
        class C {
            var x            : int;
            var someLongName : string;
            var ab           : bool;
        }
    "]]
    .assert_eq(&fmt_aligned(
        "class C { var x : int; var someLongName : string; var ab : bool; }",
    ));
}

#[test]
fn blank_line_breaks_alignment_run() {
    expect![[r"
        class C {
            var x            : int;
            var someLongName : string;

            var y              : int;
            var anotherLongOne : bool;
        }
    "]].assert_eq(&fmt_aligned("class C {\n    var x : int;\n    var someLongName : string;\n\n    var y : int;\n    var anotherLongOne : bool;\n}"));
}

#[test]
fn doc_comment_between_plain_fields_does_not_break_colon_alignment() {
    let output = fmt_aligned(
        "class C {\n    \
         var x : int;\n    \
         /** doc */\n    \
         var someLongName : string;\n}",
    );
    expect![[r"
        class C {
            var x            : int;
            /** doc */
            var someLongName : string;
        }
    "]]
    .assert_eq(&output);
}

#[test]
fn single_field_is_not_padded() {
    expect![[r"
        class C {
            var x : int;
        }
    "]]
    .assert_eq(&fmt_aligned("class C { var x : int; }"));
}

#[test]
fn alignment_accounts_for_specifiers_and_name_lists() {
    expect![[r"
        class C {
            private var a, bb : int;
            var c             : float;
        }
    "]]
    .assert_eq(&fmt_aligned(
        "class C {\n    private var a, bb : int;\n    var c : float;\n}",
    ));
}

#[test]
fn aligns_consecutive_same_line_defaults() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    private const var RESET_TIME : float; default RESET_TIME = 0.750;\n    private const var OTHER : int; default OTHER = 1;\n}",
        AnnotationPlacement::SameLine,
    );
    assert!(
        output.contains("private const var RESET_TIME : float;  default RESET_TIME = 0.750;"),
        "got:\n{output}"
    );
    let lines: Vec<&str> = output.lines().collect();
    let reset = lines
        .iter()
        .find(|l| l.contains("RESET_TIME = 0.750"))
        .expect("RESET_TIME line");
    let other = lines
        .iter()
        .find(|l| l.contains("OTHER = 1"))
        .expect("OTHER line");
    assert_eq!(
        default_keyword_col(reset, "RESET_TIME"),
        default_keyword_col(other, "OTHER"),
        "default keywords should align, got:\n{output}"
    );
}

#[test]
fn single_same_line_default_is_not_padded() {
    expect![[r"
        class C {
            private const var RESET_TIME : float;  default RESET_TIME = 0.750;
        }
    "]]
    .assert_eq(&fmt_aligned_with_default_placement(
        "class C { private const var RESET_TIME : float; default RESET_TIME = 0.750; }",
        AnnotationPlacement::SameLine,
    ));
}

#[test]
fn blank_line_breaks_default_alignment_run() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    \
         var a : int; default a = 1;\n    \
         var longName : float; default longName = 0.5;\n\
         \n    \
         var b : int; default b = 2;\n    \
         var anotherLong : float; default anotherLong = 1.0;\n}",
        AnnotationPlacement::SameLine,
    );
    let lines: Vec<&str> = output.lines().collect();
    let a = lines
        .iter()
        .find(|l| l.contains("default a = 1"))
        .expect("a line");
    let long = lines
        .iter()
        .find(|l| l.contains("default longName = 0.5"))
        .expect("longName line");
    let b = lines
        .iter()
        .find(|l| l.contains("default b = 2"))
        .expect("b line");
    let another = lines
        .iter()
        .find(|l| l.contains("default anotherLong = 1.0"))
        .expect("anotherLong line");
    assert_eq!(
        default_keyword_col(a, "a"),
        default_keyword_col(long, "longName"),
        "first run should align, got:\n{output}"
    );
    assert_eq!(
        default_keyword_col(b, "b"),
        default_keyword_col(another, "anotherLong"),
        "second run should align, got:\n{output}"
    );
    assert_ne!(
        default_keyword_col(a, "a"),
        default_keyword_col(b, "b"),
        "runs separated by a blank line should align independently, got:\n{output}"
    );
}

#[test]
fn annotated_field_excluded_from_default_alignment_run() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    \
         @addField(CClass)\n    \
         public var x : int; default x = 1;\n    \
         public var aa : int; default aa = 2;\n    \
         public var someLongName : int; default someLongName = 3;\n}",
        AnnotationPlacement::SameLine,
    );
    let lines: Vec<&str> = output.lines().collect();
    let x = lines
        .iter()
        .find(|l| l.contains("default x = 1"))
        .expect("x line");
    let aa = lines
        .iter()
        .find(|l| l.contains("default aa = 2"))
        .expect("aa line");
    let long = lines
        .iter()
        .find(|l| l.contains("default someLongName = 3"))
        .expect("someLongName line");
    assert!(
        !x.contains("var x            :"),
        "annotated field should not get colon padding, got:\n{output}"
    );
    assert_eq!(
        default_keyword_col(aa, "aa"),
        default_keyword_col(long, "someLongName"),
        "plain fields after an annotated one should still align, got:\n{output}"
    );
}

#[test]
fn own_line_default_placement_skips_default_alignment() {
    expect![[r"
        class C {
            private const var RESET_TIME : float;
            default RESET_TIME = 0.750;
            private const var OTHER : int;
            default OTHER = 1;
        }
    "]]
    .assert_eq(&fmt_aligned_with_default_placement(
        "class C {\n    \
         private const var RESET_TIME : float; default RESET_TIME = 0.750;\n    \
         private const var OTHER : int; default OTHER = 1;\n}",
        AnnotationPlacement::OwnLine,
    ));
}

#[test]
fn preserve_split_default_skips_default_alignment() {
    expect![[r#"
        class C {
            var x : int;
            default x = 1;
            var someLongName : string;
            default someLongName = "";
        }
    "#]]
    .assert_eq(&fmt_aligned(
        "class C {\n    \
         var x : int;\n    \
         default x = 1;\n    \
         var someLongName : string;\n    \
         default someLongName = \"\";\n}",
    ));
}

#[test]
fn aligns_colons_and_defaults_on_same_line_pairs() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    \
         var x : int; default x = 1;\n    \
         var someLongName : string; default someLongName = \"\";\n}",
        AnnotationPlacement::SameLine,
    );
    assert!(
        output.contains("var x            : int;"),
        "colons should align, got:\n{output}"
    );
    let lines: Vec<&str> = output.lines().collect();
    let x = lines
        .iter()
        .find(|l| l.contains("default x = 1"))
        .expect("x line");
    let long = lines
        .iter()
        .find(|l| l.contains("default someLongName"))
        .expect("someLongName line");
    assert_eq!(
        default_keyword_col(x, "x"),
        default_keyword_col(long, "someLongName"),
        "default keywords should align, got:\n{output}"
    );
    let x_semi = x.find(";").expect("semicolon");
    let x_default = default_keyword_col(x, "x");
    assert!(
        x_default - x_semi >= 3,
        "at least two spaces between `;` and `default`, got:\n{output}"
    );
    let second = fmt_aligned_with_default_placement(&output, AnnotationPlacement::SameLine);
    assert_eq!(output, second, "default alignment should be idempotent");
}

#[test]
fn aligns_three_same_line_default_pairs() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    \
         var a : int; default a = 1;\n    \
         var bb : int; default bb = 2;\n    \
         var ccc : int; default ccc = 3;\n}",
        AnnotationPlacement::SameLine,
    );
    let lines: Vec<&str> = output.lines().collect();
    let a = lines
        .iter()
        .find(|l| l.contains("default a = 1"))
        .expect("a line");
    let bb = lines
        .iter()
        .find(|l| l.contains("default bb = 2"))
        .expect("bb line");
    let ccc = lines
        .iter()
        .find(|l| l.contains("default ccc = 3"))
        .expect("ccc line");
    let a_col = default_keyword_col(a, "a");
    assert_eq!(a_col, default_keyword_col(bb, "bb"), "got:\n{output}");
    assert_eq!(a_col, default_keyword_col(ccc, "ccc"), "got:\n{output}");
}

#[test]
fn plain_field_before_default_pair_run_does_not_skew_colons() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    \
         var plainLongField : int;\n    \
         var x : int; default x = 1;\n    \
         var y : int; default y = 2;\n}",
        AnnotationPlacement::SameLine,
    );
    let lines: Vec<&str> = output.lines().collect();
    let x = lines
        .iter()
        .find(|l| l.contains("default x = 1"))
        .expect("x line");
    let y = lines
        .iter()
        .find(|l| l.contains("default y = 2"))
        .expect("y line");
    assert_eq!(
        x.find(':'),
        y.find(':'),
        "paired fields must align colons with each other, not the preceding plain field, got:\n{output}"
    );
    assert_eq!(
        default_keyword_col(x, "x"),
        default_keyword_col(y, "y"),
        "paired defaults must align, got:\n{output}"
    );
}

#[test]
fn doc_comment_between_pairs_does_not_break_default_alignment() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    \
         /** first */\n    \
         private const var ALPHA : int; default ALPHA = 6;\n    \
         /** second */\n    \
         private const var BETA : float; default BETA = 0.333;\n    \
         /** third */\n    \
         private const var GAMMA : int; default GAMMA = 1;\n}",
        AnnotationPlacement::SameLine,
    );
    expect![[r"
        class C {
            /** first */
            private const var ALPHA : int;    default ALPHA = 6;
            /** second */
            private const var BETA  : float;  default BETA = 0.333;
            /** third */
            private const var GAMMA : int;    default GAMMA = 1;
        }
    "]]
    .assert_eq(&output);
}

#[test]
fn blank_line_with_comment_still_breaks_default_alignment_run() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    \
         var a : int; default a = 1;\n    \
         var longName : float; default longName = 0.5;\n\
         \n    \
         /** new group */\n    \
         var b : int; default b = 2;\n    \
         var anotherLong : float; default anotherLong = 1.0;\n}",
        AnnotationPlacement::SameLine,
    );
    expect![[r"
        class C {
            var a        : int;    default a = 1;
            var longName : float;  default longName = 0.5;

            /** new group */
            var b           : int;    default b = 2;
            var anotherLong : float;  default anotherLong = 1.0;
        }
    "]]
    .assert_eq(&output);
}

#[test]
fn annotated_field_excluded_from_colon_alignment_run() {
    expect![[r"
        class C {
            @addField(CClass)
            public var x : int;
            public var someLongName : string;
        }
    "]].assert_eq(&fmt_aligned(
        "class C {\n    @addField(CClass)\n    public var x : int;\n    public var someLongName : string;\n}",
    ));
}
