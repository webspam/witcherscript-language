use super::fmt;

#[test]
fn error_recovery_formats_valid_stmts_around_invalid() {
    // var b has extra whitespace but is valid; var a is invalid (missing type annotation)
    let input = "function Test() {\n             var    b  : int;\n    var  a;\n}";
    let output = fmt(input);
    assert!(
        output.contains("var b : int;"),
        "valid stmt should be formatted, got:\n{output}"
    );
    assert!(
        output.contains("var  a;"),
        "invalid stmt should be preserved verbatim including semicolon, got:\n{output}"
    );
}

#[test]
fn formats_simple_function() {
    let input = "function Foo(x:int):bool{return true;}";
    let output = fmt(input);
    assert!(output.contains("function Foo(x : int) : bool {"));
    assert!(output.contains("    return true;"));
    assert!(output.contains('}'));
}

#[test]
fn add_field_annotation_stays_on_own_line() {
    let output = fmt("@addField(CR4Player) var foo : int;");
    assert_eq!(output, "@addField(CR4Player)\nvar foo : int;\n");
}

#[test]
fn annotation_sits_directly_above_declaration() {
    let field = fmt("@addField(CR4Player)\n\n\nvar foo : int;");
    assert_eq!(field, "@addField(CR4Player)\nvar foo : int;\n");

    let method = fmt("@addMethod(CR4Player)\n\n\nfunction Foo() {}");
    assert_eq!(method, "@addMethod(CR4Player)\nfunction Foo() {}\n");
}

#[test]
fn idempotent_on_valid_fixture() {
    let source = include_str!("../../../tests/fixtures/valid/basic_function.ws");
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatter should be idempotent");
}

#[test]
fn member_default_val_with_ident_value_preserved() {
    let input = "class C extends B {\n    default isPotato = OT_None;\n}";
    let output = fmt(input);
    assert!(
        output.contains("default isPotato = OT_None;"),
        "default value that is an identifier must be preserved, got:\n{output}"
    );
}

#[test]
fn local_var_init_with_ident_value_preserved() {
    let input = "function F() { var x : EOrientationTarget = OT_None; }";
    let output = fmt(input);
    assert!(
        output.contains("var x : EOrientationTarget = OT_None;"),
        "var initializer that is an identifier must be preserved, got:\n{output}"
    );
}

#[test]
fn unary_not_has_no_space_before_operand() {
    let cases = [
        "function F() { if (!thePlayer) return; }",
        "function F() { if (! thePlayer) return; }",
        "function F() { if (  ! thePlayer) return; }",
        "function F() { if (!thePlayer  ) return; }",
        "function F() { if (  !   thePlayer  ) return; }",
        "function F() { if (\n!thePlayer\n) return; }",
    ];
    for input in cases {
        let output = fmt(input);
        assert!(
            output.contains("if (!thePlayer)"),
            "unary `!` should have no space before its operand, got:\n{output}"
        );
    }
}

#[test]
fn cast_has_no_space_between_paren_and_value() {
    let cases = [
        "function F() { var x : SomeType; x = (SomeType)      someVar; }",
        "function F() { var x : SomeType; x = (  SomeType  )  someVar   ; }",
        "function F() { var x : SomeType; x = (SomeType)someVar; }",
    ];
    for input in cases {
        let output = fmt(input);
        assert!(
            output.contains("(SomeType)someVar;"),
            "cast should have no space between `)` and value, got:\n{output}"
        );
    }
}
