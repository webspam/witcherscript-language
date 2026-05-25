use rstest::rstest;

use super::super::class_body_keyword_completions;
use crate::test_support::TestDb;

fn kw_at_cursor(fixture: &str) -> Vec<&'static str> {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    class_body_keyword_completions(t.primary_doc(), pos)
}

#[rstest]
#[case::blank_position_offers_all_keywords(
    "class CExample {\n  $0\n}\n",
    &["var", "function", "event", "autobind", "private", "public", "import", "editable", "saved", "default", "defaults", "hint"],
    &[],
)]
#[case::blank_after_function_block_offers_all_keywords(
    "class CExample {\n  function Foo() {\n  }\n  $0\n}\n",
    &["var", "function", "event", "private", "editable", "defaults"],
    &[],
)]
#[case::partial_word_still_offers_keywords(
    "class C {\n  p$0\n}\n",
    &["var", "function", "private", "protected", "public"],
    &[],
)]
#[case::after_complete_specifier_plus_space_offers_filtered_keywords(
    "class C {\n  public $0\n}\n",
    &["var"],
    &["public", "private"],
)]
#[case::specifier_followed_by_valid_statement_still_offers_filtered_keywords(
    "class C {\n  public $0\n  public var valid : bool;\n}\n",
    &["var"],
    &["public", "private"],
)]
#[case::after_access_modifier_offers_decl_and_remaining_specifiers(
    "class CExample {\n  private $0\n}\n",
    &["var", "function", "autobind", "final", "latent", "editable"],
    &["private", "import"],
)]
#[case::after_editable_suppresses_func_keywords_and_const(
    "class CExample {\n  editable$0 \n}\n",
    &["var", "saved", "inlined"],
    &["const", "function", "final", "latent", "autobind", "private", "public"],
)]
#[case::saved_is_terminal_no_more_var_specifiers(
    "class CExample {\n  saved $0\n}\n",
    &["var"],
    &["editable", "const", "inlined", "private", "public"],
)]
#[case::after_access_and_saved_no_further_var_specifiers(
    "class CExample {\n  public saved$0 \n}\n",
    &["var"],
    &["inlined", "const", "editable"],
)]
#[case::after_const_only_offers_inlined(
    "class CExample {\n  const $0\n}\n",
    &["var", "inlined"],
    &["editable", "saved"],
)]
#[case::after_final_suppresses_var_and_autobind(
    "class CExample {\n  final $0\n}\n",
    &["function", "latent"],
    &["var", "autobind", "editable", "private", "public"],
)]
#[case::after_optional_no_access_only_autobind(
    "class CExample {\n  optional $0\n}\n",
    &["autobind"],
    &["private", "protected", "public", "var", "function"],
)]
#[case::after_import_suppresses_var_group_and_autobind(
    "class CExample {\n  import $0\n}\n",
    &["var", "function", "private", "final"],
    &["import", "autobind", "editable"],
)]
#[case::struct_does_not_offer_function_or_autobind(
    "struct SData {\n  $0\n}\n",
    &["var"],
    &["function", "autobind", "event", "final"],
)]
#[case::state_offers_same_as_class(
    "state SIdle in CPlayer {\n  $0\n}\n",
    &["var", "function", "event", "autobind", "private", "final"],
    &[],
)]
fn class_body_kw(
    #[case] fixture: &str,
    #[case] expected_present: &[&str],
    #[case] expected_absent: &[&str],
) {
    let result = kw_at_cursor(fixture);
    for kw in expected_present {
        assert!(
            result.contains(kw),
            "expected {kw:?} to be offered, got {result:?}"
        );
    }
    for kw in expected_absent {
        assert!(
            !result.contains(kw),
            "expected {kw:?} to NOT be offered, got {result:?}"
        );
    }
}

#[rstest]
#[case::not_offered_inside_func_block("class CExample {\n  function Foo() {\n    $0\n  }\n}\n")]
#[case::not_offered_outside_class("$0function Foo() {}\n")]
#[case::after_decl_keyword_returns_empty("class CExample {\n  private var $0\n}\n")]
fn class_body_kw_empty(#[case] fixture: &str) {
    let result = kw_at_cursor(fixture);
    assert!(result.is_empty(), "expected empty result, got {result:?}");
}
