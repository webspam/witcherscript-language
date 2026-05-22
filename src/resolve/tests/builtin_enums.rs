use super::super::{
    resolve_definition, statement_completions, type_completions, SymbolDb, WorkspaceIndex,
};
use super::{make_doc, make_index};
use crate::builtins::{load_builtins_index, BUILTIN_ENUMS_URI};
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

fn builtins_db<'a>(
    workspace: &'a WorkspaceIndex,
    base: &'a WorkspaceIndex,
    builtins: &'a WorkspaceIndex,
) -> SymbolDb<'a> {
    SymbolDb::new(workspace, base).with_builtins(builtins)
}

#[test]
fn enum_type_is_indexed_as_global_type() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let def = db
        .find_top_level("EAttackDirection")
        .expect("EAttackDirection should be indexed");
    assert_eq!(def.symbol.kind, SymbolKind::Enum);
    assert_eq!(def.uri, BUILTIN_ENUMS_URI);
}

#[test]
fn enum_variant_is_a_global_symbol() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    struct Case {
        name: &'static str,
        variant: &'static str,
    }
    let cases = [
        Case {
            name: "first enum, sole variant",
            variant: "EAIASM_GuardArea",
        },
        Case {
            name: "mid-list variant",
            variant: "AD_Back",
        },
        Case {
            name: "lowercase enum's variant",
            variant: "TreasureHunt",
        },
    ];
    for c in cases {
        let def = db
            .find_enum_variant(c.variant)
            .unwrap_or_else(|| panic!("case {}: {} should resolve", c.name, c.variant));
        assert_eq!(
            def.symbol.kind,
            SymbolKind::EnumVariant,
            "case {}: wrong kind",
            c.name
        );
        assert_eq!(def.uri, BUILTIN_ENUMS_URI, "case {}: wrong uri", c.name);
    }
}

#[test]
fn enum_types_appear_in_type_completions() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let types = db.all_types();
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
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    assert!(
        db.all_enum_variants()
            .iter()
            .any(|d| d.symbol.name == "VMT_TeleportAndMount"),
        "builtin enum variant should appear in all_enum_variants()"
    );
}

#[test]
fn goto_definition_on_enum_variant_resolves_into_builtin_file() {
    let source = concat!(
        "function Test() {\n",
        "  var d : EAttackDirection;\n",
        "  d = AD_Back;\n",
        "}\n",
    );
    let doc = make_doc(source);
    let workspace = make_index("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let builtins = load_builtins_index();
    let db = builtins_db(&workspace, &empty, &builtins);

    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 8,
        },
    )
    .expect("AD_Back should resolve");

    assert_eq!(def.uri, BUILTIN_ENUMS_URI);
    assert_eq!(def.symbol.name, "AD_Back");
    assert_eq!(def.symbol.kind, SymbolKind::EnumVariant);
}

#[test]
fn type_completions_offer_builtin_enum() {
    let source = concat!(
        "function Test() {\n",
        "  var x : EAtt\n",
        "  var y : int;\n",
        "}\n",
    );
    let doc = make_doc(source);
    let workspace = make_index("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let builtins = load_builtins_index();
    let db = builtins_db(&workspace, &empty, &builtins);

    let types = type_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 14,
        },
    );
    assert!(
        types.iter().any(|d| d.symbol.name == "EAttackDirection"),
        "type completions should offer the builtin enum; got {:?}",
        types.iter().map(|d| &d.symbol.name).collect::<Vec<_>>()
    );
}

#[test]
fn statement_completions_offer_builtin_enum_variant() {
    let source = "function Test() {\n  AD_\n}\n";
    let doc = make_doc(source);
    let workspace = make_index("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let builtins = load_builtins_index();
    let db = builtins_db(&workspace, &empty, &builtins);

    let result = statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 5,
        },
    );
    assert!(
        result
            .globals
            .iter()
            .any(|d| d.symbol.name == "AD_Front" && d.symbol.kind == SymbolKind::EnumVariant),
        "statement globals should include builtin enum variants; got {:?}",
        result
            .globals
            .iter()
            .map(|d| d.symbol.name.as_str())
            .collect::<Vec<_>>()
    );
}
