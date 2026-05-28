use super::{fmt, fmt_aligned, fmt_aligned_with_default_placement, AnnotationPlacement};

fn default_keyword_col(line: &str, field: &str) -> usize {
    line.find(&format!("default {field}"))
        .unwrap_or_else(|| panic!("line missing `default {field}`:\n{line}"))
}

#[test]
fn member_colons_not_aligned_by_default() {
    let output = fmt("class C { var x : int; var someLongName : string; }");
    assert!(output.contains("    var x : int;"), "got:\n{output}");
    assert!(
        output.contains("    var someLongName : string;"),
        "got:\n{output}"
    );
}

#[test]
fn aligns_consecutive_member_colons() {
    let output = fmt_aligned("class C { var x : int; var someLongName : string; var ab : bool; }");
    assert!(
        output.contains("    var x            : int;"),
        "got:\n{output}"
    );
    assert!(
        output.contains("    var someLongName : string;"),
        "got:\n{output}"
    );
    assert!(
        output.contains("    var ab           : bool;"),
        "got:\n{output}"
    );
}

#[test]
fn blank_line_breaks_alignment_run() {
    let output = fmt_aligned("class C {\n    var x : int;\n    var someLongName : string;\n\n    var y : int;\n    var anotherLongOne : bool;\n}");
    // First run aligns to `someLongName`.
    assert!(
        output.contains("    var x            : int;"),
        "got:\n{output}"
    );
    // Second run aligns independently to `anotherLongOne`.
    assert!(
        output.contains("    var y              : int;"),
        "got:\n{output}"
    );
    assert!(
        output.contains("    var anotherLongOne : bool;"),
        "got:\n{output}"
    );
}

#[test]
fn single_field_is_not_padded() {
    let output = fmt_aligned("class C { var x : int; }");
    assert!(output.contains("    var x : int;"), "got:\n{output}");
}

#[test]
fn alignment_accounts_for_specifiers_and_name_lists() {
    let output = fmt_aligned("class C {\n    private var a, bb : int;\n    var c : float;\n}");
    assert!(
        output.contains("    private var a, bb : int;"),
        "got:\n{output}"
    );
    assert!(
        output.contains("    var c             : float;"),
        "got:\n{output}"
    );
}

#[test]
fn aligns_consecutive_same_line_defaults() {
    let output = fmt_aligned_with_default_placement(
        "class C {\n    private const var RESET_TIME : float; default RESET_TIME = 0.750;\n    private const var OTHER : int; default OTHER = 1;\n}",
        AnnotationPlacement::SameLine,
    );
    assert!(
        output.contains("private const var RESET_TIME : float; default RESET_TIME = 0.750;"),
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
    let output = fmt_aligned_with_default_placement(
        "class C { private const var RESET_TIME : float; default RESET_TIME = 0.750; }",
        AnnotationPlacement::SameLine,
    );
    assert!(
        output.contains("private const var RESET_TIME : float;  default RESET_TIME = 0.750;"),
        "got:\n{output}"
    );
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
    let output = fmt_aligned_with_default_placement(
        "class C {\n    \
         private const var RESET_TIME : float; default RESET_TIME = 0.750;\n    \
         private const var OTHER : int; default OTHER = 1;\n}",
        AnnotationPlacement::OwnLine,
    );
    assert!(
        output.contains("private const var RESET_TIME : float;\n    default RESET_TIME = 0.750;"),
        "got:\n{output}"
    );
    assert!(
        output.contains("private const var OTHER : int;\n    default OTHER = 1;"),
        "got:\n{output}"
    );
}

#[test]
fn preserve_split_default_skips_default_alignment() {
    let output = fmt_aligned(
        "class C {\n    \
         var x : int;\n    \
         default x = 1;\n    \
         var someLongName : string;\n    \
         default someLongName = \"\";\n}",
    );
    assert!(
        output.contains("var x : int;\n    default x = 1;"),
        "got:\n{output}"
    );
    assert!(
        output.contains("var someLongName : string;\n    default someLongName = \"\";"),
        "got:\n{output}"
    );
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
fn aligned_same_line_defaults_are_idempotent() {
    let src = "class C {\n    \
               var x : int; default x = 1;\n    \
               var someLongName : string; default someLongName = \"\";\n}";
    let first = fmt_aligned_with_default_placement(src, AnnotationPlacement::SameLine);
    let second = fmt_aligned_with_default_placement(&first, AnnotationPlacement::SameLine);
    assert_eq!(first, second, "default alignment should be idempotent");
}

#[test]
fn annotated_field_excluded_from_colon_alignment_run() {
    let output = fmt_aligned(
        "class C {\n    @addField(CClass)\n    public var x : int;\n    public var someLongName : string;\n}",
    );
    assert!(
        output.contains("    public var x : int;"),
        "annotated field should not be padded, got:\n{output}"
    );
    assert!(
        output.contains("    public var someLongName : string;"),
        "got:\n{output}"
    );
}
