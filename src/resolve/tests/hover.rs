use super::super::{hover_doc, resolve_definition};
use crate::test_support::TestDb;

fn doc_at_cursor(source: &str) -> Option<String> {
    let t = TestDb::new(source);
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("definition resolves");
    hover_doc(&def, &t.db())
}

#[test]
fn plain_function_doc_is_its_own_comment() {
    let doc = doc_at_cursor(concat!(
        "// Heals the player.\n",
        "function Heal() {}\n",
        "function test() { He$0al(); }\n",
    ));
    assert_eq!(doc.as_deref(), Some("Heals the player."));
}

#[test]
fn wrap_method_doc_appends_to_base() {
    let doc = doc_at_cursor(concat!(
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
    ));
    assert_eq!(doc.as_deref(), Some("Base behaviour.\n\nWrap behaviour."));
}

#[test]
fn replace_method_doc_replaces_base() {
    let doc = doc_at_cursor(concat!(
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
    ));
    assert_eq!(doc.as_deref(), Some("Replacement behaviour."));
}
