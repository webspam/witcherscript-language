use super::super::resolve_definition;
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

const CR4_HUD_MODULE_URI: &str = "witcherscript-builtin:/CR4HudModule.ws";

#[test]
fn builtin_class_is_indexed_as_a_global_type() {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_top_level("CR4HudModule")
        .expect("CR4HudModule should be indexed");
    assert_eq!(def.symbol.kind, SymbolKind::Class);
    assert_eq!(def.uri, CR4_HUD_MODULE_URI);
    assert_eq!(def.symbol.base_class.as_deref(), Some("CHudModule"));
}

#[test]
fn builtin_class_appears_in_type_completions() {
    let t = TestDb::new("").with_builtins_index();
    let types = t.db().all_types();
    assert!(
        types.iter().any(|d| d.symbol.name == "CR4HudModule"),
        "builtin class should appear in all_types(); got {:?}",
        types.iter().map(|d| &d.symbol.name).collect::<Vec<_>>()
    );
    assert!(
        !types.iter().any(|d| d.symbol.name == "array"),
        "the generic array builtin must still be excluded from all_types()"
    );
}

#[test]
fn goto_definition_on_builtin_class_resolves_into_builtin_file() {
    let t = TestDb::new("function Test() {\n  var m : CR$04HudModule;\n}\n").with_builtins_index();
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("CR4HudModule should resolve");

    assert_eq!(def.uri, CR4_HUD_MODULE_URI);
    assert_eq!(def.symbol.name, "CR4HudModule");
}
