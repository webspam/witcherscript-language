use super::super::{resolve_all_definitions, resolve_definition};
use crate::builtins::builtin_uri;
use crate::test_support::TestDb;

#[test]
fn native_class_is_a_global_type_without_import() {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_top_level("CR4HudModule")
        .expect("CR4HudModule indexed");
    assert_eq!(def.uri, builtin_uri("CR4HudModule"));
    assert_eq!(def.symbol.base_class.as_deref(), Some("CHudModule"));
    assert!(
        t.db()
            .all_types()
            .iter()
            .any(|d| d.symbol.name == "CR4HudModule"),
        "native class should appear in type completions"
    );
}

#[test]
fn imported_native_class_resolves_to_the_import() {
    let t = TestDb::new(concat!(
        "import class CR4HudModule extends CHudModule {}\n",
        "function Test() {\n  var m : CR$04HudModule;\n}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("CR4HudModule should resolve");
    assert_eq!(def.uri, uri);
}

#[test]
fn goto_definition_on_import_name_lands_on_the_native() {
    let t =
        TestDb::new("import class CR4Hud$0Module extends CHudModule {}\n").with_builtins_index();
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(defs.len(), 1, "{defs:?}");
    assert_eq!(defs[0].uri, builtin_uri("CR4HudModule"));
    assert_eq!(defs[0].symbol.base_class.as_deref(), Some("CHudModule"));
}

#[test]
fn goto_definition_on_imported_member_name_lands_on_the_native_member() {
    let t = TestDb::new(concat!(
        "import class C2dArray extends CResource {\n",
        "  import var head$0ers : array<string>;\n",
        "}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(defs.len(), 1, "{defs:?}");
    assert_eq!(defs[0].uri, builtin_uri("C2dArray"));
    assert_eq!(defs[0].symbol.name, "headers");
}
