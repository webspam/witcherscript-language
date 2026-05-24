use rstest::rstest;

use super::super::default_or_hint_member_completions;
use crate::test_support::TestDb;

fn names_at_cursor(fixture: &str) -> Vec<String> {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    default_or_hint_member_completions(t.primary_doc(), &t.db(), pos)
        .into_iter()
        .map(|d| d.symbol.name)
        .collect()
}

#[rstest]
#[case::default_keyword(
    "class Super { private var hidden : int; }\n\
     class Sub extends Super { default $0 = 1; }\n"
)]
#[case::hint_keyword(
    "class Super { private var hidden : int; }\n\
     class Sub extends Super { hint $0 = \"tip\"; }\n"
)]
#[case::defaults_block(
    "class Super { private var hidden : int; }\n\
     class Sub extends Super { defaults { $0 = 1; } }\n"
)]
fn offers_private_inherited_field_in_default_or_hint_position(#[case] fixture: &str) {
    let names = names_at_cursor(fixture);
    assert!(
        names.iter().any(|n| n == "hidden"),
        "private inherited field should be offered, got {names:?}",
    );
}

#[rstest]
#[case::value_position_after_equals("class A { var known : int; default known = $0; }\n")]
#[case::function_body("class A { var f : int; function R() { $0 } }\n")]
fn does_not_offer_outside_default_or_hint_member_position(#[case] fixture: &str) {
    let names = names_at_cursor(fixture);
    assert!(
        names.is_empty(),
        "should not trigger default-member completion, got {names:?}",
    );
}
