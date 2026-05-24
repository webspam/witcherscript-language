use tree_sitter::Parser;

use super::{extract_symbols, SymbolKind};
use crate::line_index::LineIndex;

#[test]
fn extracts_functions_params_and_locals() {
    let source = "function Basic(owner : CObject) : bool {\n var count : int;\n return true;\n}\n";
    let tree = parse(source);
    let symbols = extract_symbols(tree.root_node(), source, &LineIndex::new(source));

    assert!(symbols
        .all()
        .iter()
        .any(|symbol| symbol.name == "Basic" && symbol.kind == SymbolKind::Function));
    assert!(symbols
        .all()
        .iter()
        .any(|symbol| symbol.name == "owner" && symbol.kind == SymbolKind::Parameter));
    assert!(symbols
        .all()
        .iter()
        .any(|symbol| symbol.name == "count" && symbol.kind == SymbolKind::Variable));
}

#[test]
fn var_decl_initializer_ident_is_not_recorded_as_local() {
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "single name with ident initializer",
            "function F() { var x : int = name; }\n",
            &["x"],
        ),
        (
            "multi-name decl with ident initializer",
            "function F() { var x, y : int = name; }\n",
            &["x", "y"],
        ),
        (
            "initializer references a prior local",
            "function F() {\n var source : int;\n var x : int = source;\n}\n",
            &["source", "x"],
        ),
    ];

    for (msg, source, expected) in cases {
        let tree = parse(source);
        let symbols = extract_symbols(tree.root_node(), source, &LineIndex::new(source));
        let vars: Vec<&str> = symbols
            .all()
            .iter()
            .filter(|s| s.kind == SymbolKind::Variable)
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(&vars[..], *expected, "{msg}");
    }
}

#[test]
fn autobind_decl_is_extracted_as_a_field() {
    let source = "class C {\n  private autobind theInput : CInputManager = single;\n}\n";
    let tree = parse(source);
    let symbols = extract_symbols(tree.root_node(), source, &LineIndex::new(source));

    let field = symbols
        .all()
        .iter()
        .find(|s| s.name == "theInput")
        .expect("autobind member must be extracted");
    assert_eq!(field.kind, SymbolKind::Field);
    assert_eq!(field.type_annotation.as_deref(), Some("CInputManager"));
}

fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_witcherscript::language())
        .expect("failed to load WitcherScript grammar");
    parser.parse(source, None).expect("failed to parse source")
}
