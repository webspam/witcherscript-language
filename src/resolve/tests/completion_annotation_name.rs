use rstest::rstest;

use super::super::annotation_name_completions;
use crate::test_support::TestDb;

#[rstest]
#[case::typing_partial_name_at_w("@w$0\n", true)]
#[case::inside_annotation_parens("@wrapMethod($0CPlayer)\n", false)]
#[case::inside_string_literal("function F() { var x : string = \"hello@$0world\"; }", false)]
#[case::inside_function_body_unclosed_string("function a(){var b:string=\"@$0", false)]
#[case::inside_function_body_closed_brace("function a(){v$0ar b:string=\"@}", false)]
#[case::bare_at_between_class_decls("\nclass a{\n\t\n}\n@$0\nclass b{function c(){}}", true)]
#[case::identifier_immediately_before_at("a@$0", false)]
fn annotation_name_completions_gate(#[case] fixture: &str, #[case] fires: bool) {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    let result = annotation_name_completions(t.primary_doc(), pos);
    if fires {
        assert!(result.is_some(), "expected gate to fire");
    } else {
        assert!(result.is_none(), "expected gate not to fire");
    }
}

#[test]
fn annotation_name_completions_fires_on_bare_at_sign() {
    let t = TestDb::new("@$0\n");
    let (_uri, pos) = t.cursor();
    let at_pos = annotation_name_completions(t.primary_doc(), pos);
    assert!(at_pos.is_some(), "should fire on bare @");
    let pos = at_pos.unwrap();
    assert_eq!(pos.line, 0, "@ position line");
    assert_eq!(pos.character, 0, "@ position character");
}
