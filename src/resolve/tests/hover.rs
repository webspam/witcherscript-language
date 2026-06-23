use rstest::rstest;

use super::super::{hover_doc, resolve_definition};
use crate::test_support::TestDb;

fn doc_at_cursor(source: &str) -> Option<String> {
    let t = TestDb::new(source);
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("definition resolves");
    hover_doc(&def, &t.db())
}

#[rstest]
#[case::plain_function_is_its_own_comment(
    concat!(
        "// Heals the player.\n",
        "function Heal() {}\n",
        "function test() { He$0al(); }\n",
    ),
    Some("Heals the player.")
)]
#[case::wrap_method_appends_to_base(
    concat!(
        "//- /base.ws\n",
        "class Foo {\n",
        "  // Base behaviour.\n",
        "  function bar() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "// Wrap behaviour.\n",
        "@wrapMethod(Foo) function bar() {}\n",
        "//- /use.ws\n",
        "function test() {\n",
        "  var f : Foo;\n",
        "  f.b$0ar();\n",
        "}\n",
    ),
    Some("Base behaviour.\n\nWrap behaviour.")
)]
#[case::replace_method_replaces_base(
    concat!(
        "//- /base.ws\n",
        "class Foo {\n",
        "  // Base behaviour.\n",
        "  function bar() {}\n",
        "}\n",
        "//- /mod.ws\n",
        "// Replacement behaviour.\n",
        "@replaceMethod(Foo) function bar() {}\n",
        "//- /use.ws\n",
        "function test() {\n",
        "  var f : Foo;\n",
        "  f.b$0ar();\n",
        "}\n",
    ),
    Some("Replacement behaviour.")
)]
fn method_doc_merges_across_declarations(#[case] source: &str, #[case] expected: Option<&str>) {
    assert_eq!(doc_at_cursor(source).as_deref(), expected);
}
