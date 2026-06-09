use rstest::rstest;

use super::SymbolKind;
use crate::test_support::TestDb;

#[test]
fn extracts_functions_params_and_locals() {
    let t = TestDb::new(
        "function Basic(owner : CObject) : bool {\n var count : int;\n return true;\n}\n",
    );
    let symbols = &t.primary_doc().symbols;

    assert!(
        symbols
            .all()
            .iter()
            .any(|symbol| symbol.name == "Basic" && symbol.kind == SymbolKind::Function)
    );
    assert!(
        symbols
            .all()
            .iter()
            .any(|symbol| symbol.name == "owner" && symbol.kind == SymbolKind::Parameter)
    );
    assert!(
        symbols
            .all()
            .iter()
            .any(|symbol| symbol.name == "count" && symbol.kind == SymbolKind::Variable)
    );
}

#[rstest]
#[case::single_name_with_ident_initializer(
    "function F() { var x : int = name; }\n",
    &["x"],
)]
#[case::multi_name_decl_with_ident_initializer(
    "function F() { var x, y : int = name; }\n",
    &["x", "y"],
)]
#[case::initializer_references_a_prior_local(
    "function F() {\n var source : int;\n var x : int = source;\n}\n",
    &["source", "x"],
)]
fn var_decl_initializer_ident_is_not_recorded_as_local(
    #[case] source: &str,
    #[case] expected: &[&str],
) {
    let t = TestDb::new(source);
    let vars: Vec<&str> = t
        .primary_doc()
        .symbols
        .all()
        .iter()
        .filter(|s| s.kind == SymbolKind::Variable)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(&vars[..], expected);
}

#[test]
fn autobind_decl_is_extracted_as_a_field() {
    let t = TestDb::new("class C {\n  private autobind theInput : CInputManager = single;\n}\n");
    let field = t
        .primary_doc()
        .symbols
        .all()
        .iter()
        .find(|s| s.name == "theInput")
        .expect("autobind member must be extracted")
        .clone();
    assert_eq!(field.kind, SymbolKind::Field);
    assert_eq!(
        field.type_annotation,
        Some(crate::types::Type::from_annotation("CInputManager"))
    );
}
