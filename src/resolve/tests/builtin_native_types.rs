use super::super::resolve_definition;
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

const NATIVE_TYPES_URI: &str = "witcherscript-builtin:/native-types.ws";

const NATIVE_TYPE_NAMES: &[&str] = &[
    "CBehTreeValBool",
    "CBehTreeValCName",
    "CBehTreeValFloat",
    "CBehTreeValInt",
    "CBehTreeValString",
];

#[test]
fn native_types_are_indexed_as_native_type_kind() {
    let t = TestDb::new("").with_builtins_index();
    for name in NATIVE_TYPE_NAMES {
        let def = t
            .db()
            .find_top_level(name)
            .unwrap_or_else(|| panic!("{name} should be indexed"));
        assert_eq!(def.symbol.kind, SymbolKind::NativeType, "{name} kind");
        assert_eq!(def.uri, NATIVE_TYPES_URI, "{name} uri");
    }
}

#[test]
fn native_types_appear_in_type_completions() {
    let t = TestDb::new("").with_builtins_index();
    let types = t.db().all_types();
    for name in NATIVE_TYPE_NAMES {
        assert!(
            types.iter().any(|d| &d.symbol.name == name),
            "{name} should appear in all_types()"
        );
    }
}

#[test]
fn goto_definition_on_native_type_resolves_into_native_types_file() {
    let t =
        TestDb::new("function Test() {\n  var v : CBehTreeValFl$0oat;\n}\n").with_builtins_index();
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("CBehTreeValFloat should resolve");
    assert_eq!(def.uri, NATIVE_TYPES_URI);
    assert_eq!(def.symbol.name, "CBehTreeValFloat");
}
