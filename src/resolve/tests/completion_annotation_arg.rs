use rstest::rstest;

use super::super::annotation_arg_completions;
use crate::test_support::TestDb;

#[rstest]
#[case::add_field(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@addField($0CPlayer)\n",
    true
)]
#[case::add_method(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@addMethod($0CPlayer)\n",
    true
)]
#[case::wrap_method(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@wrapMethod($0CPlayer)\n",
    true
)]
#[case::replace_method(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@replaceMethod($0CPlayer)\n",
    true
)]
#[case::unknown_annotation(
    "class CPlayer {}\nstruct SData {}\nenum EDir { North = 0 }\n@someUnknownAnnotation($0CPlayer)\n",
    false,
)]
fn annotation_arg_completions_offers_classes(#[case] fixture: &str, #[case] should_fire: bool) {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    let completions = annotation_arg_completions(t.primary_doc(), &t.db(), pos);

    let names: Vec<&str> = completions.iter().map(|d| d.symbol.name.as_str()).collect();
    if should_fire {
        assert!(
            names.contains(&"CPlayer"),
            "class should be offered inside parens"
        );
        assert!(
            !names.contains(&"SData"),
            "struct should not be offered inside parens"
        );
        assert!(
            !names.contains(&"EDir"),
            "enum should not be offered inside parens"
        );
    } else {
        assert!(
            completions.is_empty(),
            "unknown annotation must not get class completion"
        );
    }
}

#[test]
fn annotation_arg_completions_empty_outside_annotation() {
    let t = TestDb::new(concat!("class CPlayer {}\n", "function $0Test() {}\n",));
    let (_uri, pos) = t.cursor();
    let completions = annotation_arg_completions(t.primary_doc(), &t.db(), pos);

    assert!(
        completions.is_empty(),
        "annotation_arg_completions must not fire outside an annotation"
    );
}

#[test]
fn annotation_arg_completions_empty_after_closing_paren() {
    let t = TestDb::new("@wrapMethod(CPlayer) $0\n");
    let (_uri, pos) = t.cursor();
    let completions = annotation_arg_completions(t.primary_doc(), &t.db(), pos);

    assert!(
        completions.is_empty(),
        "annotation_arg_completions must not fire after the closing paren"
    );
}
