use rstest::rstest;

use super::super::{
    merged_global_completions, resolve_definition, statement_completions, type_completions,
};
use crate::builtins::BUILTIN_ENUMS_URI;
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

#[test]
fn enum_type_is_indexed_as_global_type() {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_top_level("EAttackDirection")
        .expect("EAttackDirection should be indexed");
    assert_eq!(def.symbol.kind, SymbolKind::Enum);
    assert_eq!(def.uri, BUILTIN_ENUMS_URI);
}

#[rstest]
#[case::first_enum_sole_variant("EAIASM_GuardArea")]
#[case::mid_list_variant("AD_Back")]
#[case::lowercase_enums_variant("TreasureHunt")]
fn enum_variant_is_a_global_symbol(#[case] variant: &str) {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_enum_variant(variant)
        .unwrap_or_else(|| panic!("{variant} should resolve"));
    assert_eq!(def.symbol.kind, SymbolKind::EnumVariant);
    assert_eq!(def.uri, BUILTIN_ENUMS_URI);
}

#[test]
fn enum_types_appear_in_type_completions() {
    let t = TestDb::new("").with_builtins_index();
    let types = t.db().all_types();
    assert!(
        types
            .iter()
            .any(|d| d.symbol.name == "EVehicleType" && d.symbol.kind == SymbolKind::Enum),
        "builtin enum should appear in all_types(); got {:?}",
        types.iter().map(|d| &d.symbol.name).collect::<Vec<_>>()
    );
    assert!(
        !types.iter().any(|d| d.symbol.name == "array"),
        "builtin array class must still be excluded from all_types()"
    );
}

#[test]
fn enum_variants_appear_in_enum_variant_globals() {
    let t = TestDb::new("").with_builtins_index();
    assert!(
        t.db()
            .all_enum_variants()
            .iter()
            .any(|d| d.symbol.name == "VMT_TeleportAndMount"),
        "builtin enum variant should appear in all_enum_variants()"
    );
}

#[test]
fn orphan_variant_bucket_is_excluded_from_type_completions() {
    let t = TestDb::new("").with_builtins_index();
    let db = t.db();
    assert!(
        !db.all_types()
            .iter()
            .any(|d| d.symbol.name == "WLSP_TooHardBasket"),
        "the synthetic orphan-variant bucket enum must not appear in all_types()"
    );
    assert!(
        db.all_enum_variants()
            .iter()
            .any(|d| d.symbol.name == "FLAG_OnlyActors"),
        "orphan enum variants must still appear in all_enum_variants()"
    );
}

#[test]
fn goto_definition_on_enum_variant_resolves_into_builtin_file() {
    let t = TestDb::new(concat!(
        "function Test() {\n",
        "  var d : EAttackDirection;\n",
        "  d = AD$0_Back;\n",
        "}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let def =
        resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("AD_Back should resolve");

    assert_eq!(def.uri, BUILTIN_ENUMS_URI);
    assert_eq!(def.symbol.name, "AD_Back");
    assert_eq!(def.symbol.kind, SymbolKind::EnumVariant);
}

#[test]
fn type_completions_offer_builtin_enum() {
    let t = TestDb::new(concat!(
        "function Test() {\n",
        "  var x : EAtt$0\n",
        "  var y : int;\n",
        "}\n",
    ))
    .with_builtins_index();
    let (_uri, pos) = t.cursor();
    let types = type_completions(t.primary_doc(), &t.db(), pos);
    assert!(
        types.iter().any(|d| d.symbol.name == "EAttackDirection"),
        "type completions should offer the builtin enum; got {:?}",
        types.iter().map(|d| &d.symbol.name).collect::<Vec<_>>()
    );
}

#[test]
fn statement_completions_offer_builtin_enum_variant() {
    let t = TestDb::new("function Test() {\n  AD_$0\n}\n").with_builtins_index();
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert!(result.needs_globals);
    let globals = merged_global_completions(&t.db());
    assert!(
        globals
            .iter()
            .any(|d| d.symbol.name == "AD_Front" && d.symbol.kind == SymbolKind::EnumVariant),
        "statement globals should include builtin enum variants; got {:?}",
        globals
            .iter()
            .map(|d| d.symbol.name.as_str())
            .collect::<Vec<_>>()
    );
}
