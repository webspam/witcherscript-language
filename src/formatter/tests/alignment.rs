use super::{fmt, fmt_aligned, fmt_aligned_with_annotation_placement, AnnotationPlacement};

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
fn aligns_same_line_add_field_with_adjacent_fields() {
    let output = fmt_aligned_with_annotation_placement(
        "class C {\n    @addField(CClass) public var x : int;\n    public var someLongName : string;\n}",
        AnnotationPlacement::SameLine,
    );
    let x_colon = output
        .lines()
        .find(|l| l.contains("@addField(CClass) public var x"))
        .and_then(|l| l.find(':'))
        .expect("addField line");
    let long_colon = output
        .lines()
        .find(|l| l.contains("someLongName"))
        .and_then(|l| l.find(':'))
        .expect("long name line");
    assert_eq!(x_colon, long_colon, "colons should align, got:\n{output}");
}

#[test]
fn own_line_add_field_excluded_from_colon_alignment_run() {
    let output = fmt_aligned(
        "class C {\n    @addField(CClass)\n    public var x : int;\n    public var someLongName : string;\n}",
    );
    assert!(
        output.contains("    public var x : int;"),
        "own-line addField field should not be padded, got:\n{output}"
    );
    assert!(
        output.contains("    public var someLongName : string;"),
        "got:\n{output}"
    );
}
