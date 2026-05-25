use rstest::rstest;

use super::super::script_body_completions;
use crate::test_support::TestDb;

fn kw_at_cursor(fixture: &str) -> Vec<&'static str> {
    let t = TestDb::new(fixture);
    let (_uri, pos) = t.cursor();
    script_body_completions(t.primary_doc(), pos)
}

#[rstest]
#[case::blank_offers_all_starters(
    "$0\n",
    &["class", "state", "struct", "enum", "function", "var", "import", "abstract", "statemachine", "final", "latent", "cleanup", "entry", "exec", "quest", "reward", "storyscene", "timer"],
    &["private", "protected", "public"],
)]
#[case::blank_between_decls_offers_all(
    "class A {}\n$0\nclass B {}\n",
    &["class", "function", "import", "final"],
    &["private"],
)]
#[case::blank_offers_modding_annotations(
    "$0\n",
    &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"],
    &[],
)]
#[case::annotations_not_offered_after_specifier_import(
    "import $0\n",
    &[],
    &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"],
)]
#[case::annotations_not_offered_after_specifier_final(
    "final $0\n",
    &[],
    &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"],
)]
#[case::annotations_not_offered_after_specifier_abstract(
    "abstract $0\n",
    &[],
    &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"],
)]
#[case::annotations_not_offered_after_specifier_statemachine(
    "statemachine $0\n",
    &[],
    &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"],
)]
#[case::after_add_field_offers_access_modifiers(
    "@addField(CName)\n$0\n",
    &["private", "protected", "public"],
    &[],
)]
#[case::after_add_method_offers_access_modifiers(
    "@addMethod(CName)\n$0\n",
    &["private", "protected", "public"],
    &[],
)]
#[case::after_add_field_offers_var_starters_only(
    "@addField(CName)\n$0\n",
    &["editable", "saved", "const", "inlined", "var"],
    &["function"],
)]
#[case::after_add_field_excludes_top_level_keywords(
    "@addField(CName)\n$0\n",
    &[],
    &["class", "state", "struct", "enum", "import", "statemachine", "abstract", "addField", "addMethod", "wrapMethod", "replaceMethod"],
)]
#[case::after_add_method_excludes_top_level_keywords(
    "@addMethod(CName)\n$0\n",
    &[],
    &["class", "state", "struct", "enum", "import", "statemachine", "abstract", "addField", "addMethod", "wrapMethod", "replaceMethod"],
)]
#[case::after_wrap_method_no_access_modifiers(
    "@wrapMethod(CName)\n$0\n",
    &[],
    &["private", "protected", "public"],
)]
#[case::after_replace_method_no_access_modifiers(
    "@replaceMethod(CName)\n$0\n",
    &[],
    &["private", "protected", "public"],
)]
#[case::access_modifiers_gated_off_after_a_specifier(
    "@addMethod(CName)\nfinal $0\n",
    &[],
    &["private", "protected", "public"],
)]
#[case::after_import_filters_to_compatible_decls(
    "import $0\n",
    &["class", "state", "struct", "function", "abstract", "statemachine", "final", "latent"],
    &["import", "enum", "var", "private", "protected", "public"],
)]
#[case::after_statemachine_only_class_path(
    "statemachine $0\n",
    &["class", "abstract", "import"],
    &["state", "struct", "enum", "function", "var", "private"],
)]
#[case::after_abstract_offers_class_or_state(
    "abstract $0\n",
    &["class", "state", "import"],
    &["statemachine", "struct", "function", "abstract", "private"],
)]
#[case::after_final_drops_latent_ordering(
    "final $0\n",
    &["function", "latent", "timer"],
    &["final", "private", "class"],
)]
#[case::after_latent_offers_flavours_and_function(
    "latent $0\n",
    &["function", "quest", "storyscene"],
    &["latent", "final", "private", "class"],
)]
#[case::after_flavour_only_function(
    "timer $0\n",
    &["function"],
    &["timer", "quest", "final", "latent", "private"],
)]
#[case::partial_word_still_offers_keywords(
    "cl$0\n",
    &["class", "function", "import", "final"],
    &["private"],
)]
#[case::offered_after_annotation_on_next_line(
    "@addMethod(CPlayer)\n$0\n",
    &["function", "final", "latent"],
    &[],
)]
#[case::chain_statemachine_import_abstract_only_class(
    "statemachine import abstract $0\n",
    &["class"],
    &["state", "struct", "function", "statemachine", "abstract", "import"],
)]
#[case::chain_import_final_only_function(
    "import final $0\n",
    &["function", "latent", "timer"],
    &["class", "final", "import", "private"],
)]
fn script_kw(
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
#[case::not_offered_inside_class_body("class C {\n  $0\n}\n")]
#[case::not_offered_inside_func_block("function F() {\n  $0\n}\n")]
#[case::annotations_not_offered_inside_class_body("class C {\n  $0\n}\n")]
#[case::after_decl_keyword_returns_empty("class $0\n")]
#[case::after_function_decl_keyword_returns_empty("function $0\n")]
fn script_kw_empty(#[case] fixture: &str) {
    let result = kw_at_cursor(fixture);
    assert!(result.is_empty(), "expected empty result, got {result:?}");
}
