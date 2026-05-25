use rstest::rstest;

use super::super::resolve_definition;
use crate::test_support::TestDb;

#[rstest]
#[case::single_call(
    concat!(
        "class Duck {\n",
        "  public function Quack() {}\n",
        "}\n",
        "function ReturnsDuck() : Duck {}\n",
        "function Test() {\n",
        "  ReturnsDuck().$0Quack();\n",
        "}\n",
    ),
    "Quack",
    "Duck",
)]
#[case::two_level_chain(
    concat!(
        "class Nest {\n",
        "  public function Count() : int {}\n",
        "}\n",
        "class Duck {\n",
        "  public function GetNest() : Nest {}\n",
        "}\n",
        "function ReturnsDuck() : Duck {}\n",
        "function Test() {\n",
        "  ReturnsDuck().GetNest().$0Count();\n",
        "}\n",
    ),
    "Count",
    "Nest",
)]
fn resolves_method_via_return_type(
    #[case] fixture: &str,
    #[case] expected_name: &str,
    #[case] expected_container: &str,
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let definition = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("method should resolve via return type");
    assert_eq!(definition.symbol.name, expected_name);
    let container_id = definition.symbol.container.expect("method has a container");
    let container = t
        .doc_for(&uri)
        .symbols
        .by_id(container_id)
        .expect("container exists");
    assert_eq!(container.name, expected_container);
}
