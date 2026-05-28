use super::{fmt, fmt_aligned, fmt_aligned_with_default_placement, AnnotationPlacement};

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
    let reset_col = reset.find("default RESET_TIME").expect("default keyword");
    let other_col = other.find("default OTHER").expect("default keyword");
    assert_eq!(
        reset_col, other_col,
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
        output.contains("private const var RESET_TIME : float; default RESET_TIME = 0.750;"),
        "got:\n{output}"
    );
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
