use rstest::rstest;

use crate::symbols::AccessLevel;
use crate::test_support::TestDb;

#[rstest]
#[case::three_named_in_order(
    "function Find(findName : string, range : float, shouldScanAllObjects : bool) : int {}",
    "Find",
    &["findName", "range", "shouldScanAllObjects"],
)]
#[case::zero_params("function NoArgs() {}", "NoArgs", &[])]
#[case::skips_optional(
    "function Find(name : string, optional range : float) : int {}",
    "Find",
    &["name"],
)]
#[case::multi_name_group(
    "function Multi(a, b : int, c : string) {}",
    "Multi",
    &["a", "b", "c"],
)]
fn parameters_of_top_level(
    #[case] source: &str,
    #[case] callable: &str,
    #[case] expected: &[&str],
) {
    let t = TestDb::new(source);
    let db = t.db();
    let def = db
        .find_top_level(callable)
        .unwrap_or_else(|| panic!("{callable} should be indexed"));
    let params = db.parameters_of(&def.uri, def.symbol.id);
    let expected: Vec<String> = expected.iter().map(std::string::ToString::to_string).collect();
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
    let params = db.parameters_of(&def.uri, def.symbol.id);
    let expected: Vec<String> = expected.iter().map(std::string::ToString::to_string).collect();
    assert_eq!(params, expected);
}
