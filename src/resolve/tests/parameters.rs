use rstest::rstest;

use crate::symbols::AccessLevel;
use crate::test_support::TestDb;

#[rstest]
#[case::three_named_in_order(
    "function Find(findName : string, range : float, shouldScanAllObjects : bool) : int {}",
    "Find",
    &[("findName", false), ("range", false), ("shouldScanAllObjects", false)],
)]
#[case::zero_params("function NoArgs() {}", "NoArgs", &[])]
#[case::optional_is_flagged(
    "function Find(name : string, optional range : float) : int {}",
    "Find",
    &[("name", false), ("range", true)],
)]
#[case::multi_name_group(
    "function Multi(a, b : int, c : string) {}",
    "Multi",
    &[("a", false), ("b", false), ("c", false)],
)]
fn parameters_of_top_level(
    #[case] source: &str,
    #[case] callable: &str,
    #[case] expected: &[(&str, bool)],
) {
    let t = TestDb::new(source);
    let db = t.db();
    let def = db
        .find_top_level(callable)
        .unwrap_or_else(|| panic!("{callable} should be indexed"));
    let params: Vec<(String, bool)> = db
        .display_parameters_of(&def)
        .into_iter()
        .map(|p| (p.name, p.specifiers.is_optional()))
        .collect();
    let expected: Vec<(String, bool)> = expected
        .iter()
        .map(|(name, optional)| ((*name).to_string(), *optional))
        .collect();
    assert_eq!(params, expected);
}

#[rstest]
#[case::method(
    "class CPlayer { function GetHealth(modifier : float) : int {} }",
    "CPlayer",
    "GetHealth",
    &["modifier"],
)]
#[case::event(
    "class C { event OnSpawn(spawnData : int) {} }",
    "C",
    "OnSpawn",
    &["spawnData"],
)]
fn parameters_of_class_member(
    #[case] source: &str,
    #[case] class: &str,
    #[case] member: &str,
    #[case] expected: &[&str],
) {
    let t = TestDb::new(source);
    let db = t.db();
    let def = db
        .find_member(class, member, AccessLevel::Public)
        .unwrap_or_else(|| panic!("{class}.{member} should be indexed"));
    let params: Vec<String> = db
        .display_parameters_of(&def)
        .into_iter()
        .map(|p| p.name)
        .collect();
    let expected: Vec<String> = expected
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    assert_eq!(params, expected);
}
