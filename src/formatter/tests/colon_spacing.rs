use super::{fmt, fmt_compact_colon};

#[test]
fn compact_colon_local_var() {
    let input = "function F() { var x : int; }";
    let output = fmt_compact_colon(input);
    assert!(output.contains("var x: int;"), "got:\n{output}");
}

#[test]
fn compact_colon_member_var() {
    let input = "class C { var x : int; }";
    let output = fmt_compact_colon(input);
    assert!(output.contains("var x: int;"), "got:\n{output}");
}

#[test]
fn compact_colon_func_param() {
    let input = "function F(x : int, y : bool) {}";
    let output = fmt_compact_colon(input);
    assert!(output.contains("(x: int, y: bool)"), "got:\n{output}");
}

#[test]
fn compact_colon_return_type() {
    let input = "function F() : bool { return true; }";
    let output = fmt_compact_colon(input);
    assert!(output.contains("function F(): bool"), "got:\n{output}");
}

#[test]
fn default_colon_style_unchanged() {
    let output = fmt("function F(x : int) : bool { var y : int; return true; }");
    assert!(output.contains("(x : int)"), "got:\n{output}");
    assert!(output.contains(") : bool"), "got:\n{output}");
    assert!(output.contains("var y : int;"), "got:\n{output}");
}
