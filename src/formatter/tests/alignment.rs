use super::{fmt, fmt_aligned};

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
