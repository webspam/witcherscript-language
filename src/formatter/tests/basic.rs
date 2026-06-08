use expect_test::{Expect, expect};
use rstest::rstest;

use super::{AnnotationPlacement, fmt, fmt_with_annotation_placement, fmt_with_default_placement};

#[test]
fn error_recovery_formats_valid_stmts_around_invalid() {
    // var b has extra whitespace but is valid; var a is invalid (missing type annotation)
    expect![[r#"
        function Test() {
            var b : int;
            var  a;
        }
    "#]]
    .assert_eq(&fmt(
        "function Test() {\n             var    b  : int;\n    var  a;\n}",
    ));
}

#[test]
fn formats_simple_function() {
    expect![[r#"
        function Foo(x : int) : bool {
            return true;
        }
    "#]]
    .assert_eq(&fmt("function Foo(x:int):bool{return true;}"));
}

#[test]
fn add_field_annotation_stays_on_own_line() {
    expect![[r#"
        @addField(CR4Player)
        var foo : int;
    "#]]
    .assert_eq(&fmt_with_annotation_placement(
        "@addField(CR4Player) var foo : int;",
        AnnotationPlacement::OwnLine,
    ));
}

#[rstest]
#[case::same("@addField(CClass) public var someField : bool;", expect![[r#"
    @addField(CClass) public var someField : bool;
"#]])]
#[case::messy("@addField(  CClass  )   public   var   someField   :  bool  ;", expect![[r#"
    @addField(CClass) public var someField : bool;
"#]])]
#[case::split("@addField(CClass)\npublic var someField : bool;", expect![[r#"
    @addField(CClass)
    public var someField : bool;
"#]])]
fn preserve_annotation_line_break(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}

#[rstest]
#[case::same("@addField(CClass) public var someField : bool;", expect![[r#"
    @addField(CClass) public var someField : bool;
"#]])]
#[case::messy("@addField(  CClass  )   public   var   someField   :  bool  ;", expect![[r#"
    @addField(CClass) public var someField : bool;
"#]])]
#[case::split("@addField(CClass)\npublic var someField : bool;", expect![[r#"
    @addField(CClass) public var someField : bool;
"#]])]
fn same_line_annotation_placement(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt_with_annotation_placement(
        input,
        AnnotationPlacement::SameLine,
    ));
}

#[test]
fn annotation_sits_directly_above_declaration() {
    expect![[r#"
        @addField(CR4Player)
        var foo : int;
    "#]]
    .assert_eq(&fmt_with_annotation_placement(
        "@addField(CR4Player)\n\n\nvar foo : int;",
        AnnotationPlacement::OwnLine,
    ));
    expect![[r#"
        @addMethod(CR4Player)
        function Foo() {}
    "#]]
    .assert_eq(&fmt("@addMethod(CR4Player)\n\n\nfunction Foo() {}"));
}

#[test]
fn add_method_annotation_ignores_placement_setting() {
    let input = "@addMethod(CR4Player) function Foo() {}";
    expect![[r#"
        @addMethod(CR4Player)
        function Foo() {}
    "#]]
    .assert_eq(&fmt_with_annotation_placement(
        input,
        AnnotationPlacement::SameLine,
    ));
    expect![[r#"
        @addMethod(CR4Player)
        function Foo() {}
    "#]]
    .assert_eq(&fmt(input));
}

#[test]
fn idempotent_on_valid_fixture() {
    let source = include_str!("../../../tests/fixtures/valid/basic_function.ws");
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatter should be idempotent");
}

#[rstest]
#[case::same(
    "class C { private const var RESET_TIME : float; default RESET_TIME = 0.750; }",
    expect![[r#"
        class C {
            private const var RESET_TIME : float;  default RESET_TIME = 0.750;
        }
    "#]]
)]
#[case::split(
    "class C {\n    private const var RESET_TIME : float;\n    default RESET_TIME = 0.750;\n}",
    expect![[r#"
        class C {
            private const var RESET_TIME : float;
            default RESET_TIME = 0.750;
        }
    "#]]
)]
fn preserve_default_placement(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}

#[rstest]
#[case::same(
    "class C { private const var RESET_TIME : float; default RESET_TIME = 0.750; }",
    expect![[r#"
        class C {
            private const var RESET_TIME : float;  default RESET_TIME = 0.750;
        }
    "#]]
)]
#[case::split(
    "class C {\n    private const var RESET_TIME : float;\n    default RESET_TIME = 0.750;\n}",
    expect![[r#"
        class C {
            private const var RESET_TIME : float;  default RESET_TIME = 0.750;
        }
    "#]]
)]
fn same_line_default_placement(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt_with_default_placement(
        input,
        AnnotationPlacement::SameLine,
    ));
}

#[test]
fn own_line_default_placement() {
    expect![[r#"
        class C {
            private const var RESET_TIME : float;
            default RESET_TIME = 0.750;
        }
    "#]]
    .assert_eq(&fmt_with_default_placement(
        "class C { private const var RESET_TIME : float; default RESET_TIME = 0.750; }",
        AnnotationPlacement::OwnLine,
    ));
}

#[test]
fn default_only_merges_when_ident_matches() {
    expect![[r#"
        class C {
            private const var RESET_TIME : float;
            default OTHER = 1;
        }
    "#]]
    .assert_eq(&fmt_with_default_placement(
        "class C {\n    private const var RESET_TIME : float;\n    default OTHER = 1;\n}",
        AnnotationPlacement::SameLine,
    ));
}

#[test]
fn annotated_field_default_merges_under_same_line() {
    expect![[r#"
        class C {
            @addField(CClass)
            public var x : int;  default x = 1;
        }
    "#]]
    .assert_eq(&fmt_with_default_placement(
        "class C {\n    @addField(CClass)\n    public var x : int;\n    default x = 1;\n}",
        AnnotationPlacement::SameLine,
    ));
}

#[test]
fn commented_field_with_same_line_default_not_merged() {
    expect![[r#"
        class C {
            var x /* c */ : int;
            default x = 1;
        }
    "#]]
    .assert_eq(&fmt_with_default_placement(
        "class C {\n    var x /* c */ : int;\n    default x = 1;\n}",
        AnnotationPlacement::SameLine,
    ));
}

#[test]
fn member_default_val_with_ident_value_preserved() {
    expect![[r#"
        class C extends B {
            default isPotato = OT_None;
        }
    "#]]
    .assert_eq(&fmt(
        "class C extends B {\n    default isPotato = OT_None;\n}",
    ));
}

#[test]
fn local_var_init_with_ident_value_preserved() {
    expect![[r#"
        function F() {
            var x : EOrientationTarget = OT_None;
        }
    "#]]
    .assert_eq(&fmt(
        "function F() { var x : EOrientationTarget = OT_None; }",
    ));
}

#[rstest]
#[case::no_space("function F() { if (!thePlayer) return; }", expect![[r#"
    function F() {
        if (!thePlayer) return;
    }
"#]])]
#[case::space_after("function F() { if (! thePlayer) return; }", expect![[r#"
    function F() {
        if (!thePlayer) return;
    }
"#]])]
#[case::leading_space("function F() { if (  ! thePlayer) return; }", expect![[r#"
    function F() {
        if (!thePlayer) return;
    }
"#]])]
#[case::trailing_space("function F() { if (!thePlayer  ) return; }", expect![[r#"
    function F() {
        if (!thePlayer) return;
    }
"#]])]
#[case::spaces_both("function F() { if (  !   thePlayer  ) return; }", expect![[r#"
    function F() {
        if (!thePlayer) return;
    }
"#]])]
#[case::newlines("function F() { if (\n!thePlayer\n) return; }", expect![[r#"
    function F() {
        if (!thePlayer) return;
    }
"#]])]
fn unary_not_has_no_space_before_operand(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}

#[rstest]
#[case::tight("var x : array<CComponent>;", expect![[r#"
    var x : array<CComponent>;
"#]])]
#[case::spaced("var x : array   <   CComponent   >;", expect![[r#"
    var x : array<CComponent>;
"#]])]
#[case::single_space("var x : array <CComponent>;", expect![[r#"
    var x : array<CComponent>;
"#]])]
fn generic_type_has_no_space_around_angle_brackets(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}

#[rstest]
#[case::spaces_after(
    "function F() { var x : SomeType; x = (SomeType)      someVar; }",
    expect![[r#"
        function F() {
            var x : SomeType;
            x = (SomeType)someVar;
        }
    "#]]
)]
#[case::spaces_around(
    "function F() { var x : SomeType; x = (  SomeType  )  someVar   ; }",
    expect![[r#"
        function F() {
            var x : SomeType;
            x = (SomeType)someVar;
        }
    "#]]
)]
#[case::tight("function F() { var x : SomeType; x = (SomeType)someVar; }", expect![[r#"
    function F() {
        var x : SomeType;
        x = (SomeType)someVar;
    }
"#]])]
fn cast_has_no_space_between_paren_and_value(#[case] input: &str, #[case] expected: Expect) {
    expected.assert_eq(&fmt(input));
}
