use super::fmt;

#[test]
fn trailing_comment_on_error_member_var_preserved() {
    let input = "class C {\n    var x : int // trailing comment\n}";
    let output = fmt(input);
    assert!(
        output.contains("// trailing comment"),
        "trailing comment on error member_var_decl must be preserved, got:\n{output}"
    );
    assert!(
        !output.contains("// trailing comment;"),
        "spurious semicolon must not be appended after the comment, got:\n{output}"
    );
}

#[test]
fn trailing_comment_on_member_default_val_preserved() {
    let input = "class C {\n    var x : int;\n    default x = OT_None // keep me\n    ;\n}";
    let output = fmt(input);
    assert!(
        output.contains("// keep me"),
        "trailing comment on member_default_val must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_before_param_preserved() {
    let input = "function lossy(/* comment */ param1 : bool) {}";
    let output = fmt(input);
    assert!(
        output.contains("/* comment */"),
        "leading comment inside param list must be preserved, got:\n{output}"
    );
    assert!(
        output.contains("param1"),
        "param name must still be present, got:\n{output}"
    );
}

#[test]
fn comment_between_params_preserved() {
    let input = "function F(a : int, /* mid */ b : bool) {}";
    let output = fmt(input);
    assert!(
        output.contains("/* mid */"),
        "inter-param comment must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_in_enum_body_preserved() {
    let input = "enum EKind {\n    // a comment\n    A,\n    B\n}";
    let output = fmt(input);
    assert!(
        output.contains("// a comment"),
        "comment inside enum body must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_in_defaults_block_preserved() {
    let input = "class C {\n    defaults {\n        // a comment\n        x = 1;\n    }\n}";
    let output = fmt(input);
    assert!(
        output.contains("// a comment"),
        "comment inside defaults block must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_trailing_param_preserved() {
    let input = "function F(a : int /* trailing */) {}";
    let output = fmt(input);
    assert!(
        output.contains("/* trailing */"),
        "trailing comment after last param must be preserved, got:\n{output}"
    );
}

#[test]
fn comment_between_params_comma_position() {
    let input = "function F(a : bool /*b*/,/*a*/ i : int) {}";
    let output = fmt(input);
    assert!(
        output.contains("/*b*/"),
        "first comment dropped, got:\n{output}"
    );
    assert!(
        output.contains("/*a*/"),
        "second comment dropped, got:\n{output}"
    );
    assert!(
        !output.contains("bool,"),
        "comma must not appear immediately after type (before trailing comments), got:\n{output}"
    );
}

#[test]
fn comment_between_params_keeps_source_comma_position() {
    // /*f*/ appears before the comma in source; /*g*/ appears after. The formatter
    // must not collapse both onto one side of the comma.
    let input = "function lossy(   /*a*/   a   /*b*/  /*c*/  :/*d*//*e*/bool/*f*/,/*g*/i:int) {}";
    let output = fmt(input);
    assert!(
        output.contains("/*f*/, /*g*/"),
        "comma must sit between /*f*/ and /*g*/ as in source, got:\n{output}"
    );
}
