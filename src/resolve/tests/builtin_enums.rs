use rstest::rstest;

use super::super::{merged_global_completions, resolve_definition, statement_completions};
use crate::builtins::builtin_uri;
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

#[test]
fn native_enum_type_is_usable_without_import() {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_top_level("EAttackDirection")
        .expect("EAttackDirection indexed");
    assert_eq!(def.uri, builtin_uri("EAttackDirection"));
    let types = t.db().all_types();
    assert!(
        types.iter().any(|d| d.symbol.name == "EAttackDirection"),
        "native enum should appear in type completions"
    );
    assert!(
        !types.iter().any(|d| d.symbol.name == "array"),
        "builtin array class must still be excluded from all_types()"
    );
}

#[rstest]
#[case::first_enum_sole_member("EAIASM_GuardArea", "EAIAreaSelectionMode")]
#[case::mid_list_member("AD_Back", "EAttackDirection")]
#[case::lowercase_enums_member("TreasureHunt", "eQuestType")]
fn native_enum_member_is_a_global_symbol(#[case] member: &str, #[case] owner: &str) {
    let t = TestDb::new("").with_builtins_index();
    let def = t
        .db()
        .find_enum_member(member)
        .unwrap_or_else(|| panic!("{member} should resolve"));
    assert_eq!(def.symbol.kind, SymbolKind::EnumMember);
    assert_eq!(def.symbol.container_name.as_deref(), Some(owner));
    assert_eq!(def.uri, builtin_uri(owner));
}

#[test]
fn enum_members_appear_in_enum_member_globals() {
    let t = TestDb::new("").with_builtins_index();
    assert!(
        t.db()
            .all_enum_members()
            .iter()
            .any(|d| d.symbol.name == "VMT_TeleportAndMount"),
        "builtin enum member should appear in all_enum_members()"
    );
}

#[test]
fn goto_definition_on_enum_member_resolves_into_its_enum_file() {
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

    assert_eq!(def.uri, builtin_uri("EAttackDirection"));
    assert_eq!(def.symbol.name, "AD_Back");
    assert_eq!(def.symbol.kind, SymbolKind::EnumMember);
}

#[test]
fn statement_completions_offer_builtin_enum_member() {
    let t = TestDb::new("function Test() {\n  AD_$0\n}\n").with_builtins_index();
    let (uri, pos) = t.cursor();
    let result = statement_completions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert!(result.needs_globals);
    let globals = merged_global_completions(&t.db());
    assert!(
        globals
            .iter()
            .any(|d| d.symbol.name == "AD_Front" && d.symbol.kind == SymbolKind::EnumMember),
        "statement globals should include builtin enum members; got {:?}",
        globals
            .iter()
            .map(|d| d.symbol.name.as_str())
            .collect::<Vec<_>>()
    );
}
