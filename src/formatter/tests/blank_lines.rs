use super::fmt;

#[test]
fn blank_lines_after_annotated_decl_capped_at_one() {
    let output = fmt("@addField(CR4Player)\nvar foo : int;\n\n\n\nvar bar : int;");
    assert_eq!(
        output,
        "@addField(CR4Player)\nvar foo : int;\n\nvar bar : int;\n"
    );
}

#[test]
fn preserves_single_blank_line_in_body() {
    let input = "function F() {\n    a();\n\n    b();\n}";
    let output = fmt(input);
    assert!(
        output.contains("a();\n\n    b();"),
        "blank line should be preserved"
    );
}

#[test]
fn collapses_multiple_blank_lines_to_one() {
    let input = "function F() {\n    a();\n\n\n    b();\n}";
    let output = fmt(input);
    assert!(
        output.contains("a();\n\n    b();"),
        "multiple blank lines should collapse to one"
    );
    assert!(
        !output.contains("a();\n\n\n"),
        "should not have two consecutive blank lines"
    );
}

#[test]
fn blank_line_between_class_fields_preserved() {
    let input = "class C extends B {\n    var a : int;\n\n    var b : int;\n}";
    let output = fmt(input);
    assert!(
        output.contains("var a : int;\n\n    var b : int;"),
        "blank line between fields should be preserved, got:\n{output}"
    );
}

#[test]
fn blank_line_at_class_start_preserved() {
    let input = "class C extends B {\n\n    var a : int;\n}";
    let output = fmt(input);
    assert!(
        output.contains("{\n\n    var a : int;"),
        "leading blank line inside class body should be preserved, got:\n{output}"
    );
}

#[test]
fn multiple_blank_lines_in_class_condensed_to_one() {
    let input = "class C extends B {\n    var a : int;\n\n\n    var b : int;\n}";
    let output = fmt(input);
    assert!(
        output.contains("var a : int;\n\n    var b : int;"),
        "multiple blank lines should collapse to one, got:\n{output}"
    );
    assert!(
        !output.contains("var a : int;\n\n\n"),
        "should not have two consecutive blank lines, got:\n{output}"
    );
}

#[test]
fn no_blank_line_between_adjacent_class_fields() {
    let input = "class C extends B {\n    var a : int;\n    var b : int;\n}";
    let output = fmt(input);
    assert!(
        !output.contains("var a : int;\n\n"),
        "adjacent fields with no blank line in source should not gain one, got:\n{output}"
    );
}

#[test]
fn no_blank_line_between_adjacent_abstract_methods() {
    let input = "abstract class C {\n    function Foo();\n    function Bar();\n}";
    let output = fmt(input);
    assert!(
        !output.contains("function Foo();\n\n"),
        "adjacent abstract methods should not gain a blank line, got:\n{output}"
    );
}

#[test]
fn blank_line_between_abstract_methods_preserved() {
    let input = "abstract class C {\n    function Foo();\n\n    function Bar();\n}";
    let output = fmt(input);
    assert!(
        output.contains("function Foo();\n\n    function Bar();"),
        "explicit blank line between abstract methods should be preserved, got:\n{output}"
    );
}

#[test]
fn blank_line_forced_between_abstract_and_bodied_methods() {
    let input = "abstract class C {\n    function Foo();\n    function Bar() {}\n}";
    let output = fmt(input);
    assert!(
        output.contains("function Foo();\n\n    function Bar() {}"),
        "bodied method following abstract one should still get a blank line, got:\n{output}"
    );
}

#[test]
fn blank_line_forced_between_bodied_and_abstract_methods() {
    let input = "abstract class C {\n    function Foo() {}\n    function Bar();\n}";
    let output = fmt(input);
    assert!(
        output.contains("function Foo() {}\n\n    function Bar();"),
        "abstract method following bodied one should still get a blank line, got:\n{output}"
    );
}
