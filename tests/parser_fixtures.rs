use std::fs;
use std::path::{Path, PathBuf};

use witcherscript_parser::diagnostics::collect_diagnostics;

#[test]
fn valid_fixture_files_parse_without_diagnostics() {
    for path in fixture_files("valid") {
        let source = fs::read_to_string(&path).expect("failed to read valid fixture");
        let tree = parse(&source);
        let diagnostics = collect_diagnostics(tree.root_node(), &source);

        assert!(
            diagnostics.is_empty(),
            "{} should parse cleanly, got diagnostics: {diagnostics:#?}",
            path.display()
        );
    }
}

#[test]
fn invalid_fixture_files_report_tree_sitter_errors() {
    for path in fixture_files("invalid") {
        let source = fs::read_to_string(&path).expect("failed to read invalid fixture");
        let tree = parse(&source);
        let diagnostics = collect_diagnostics(tree.root_node(), &source);

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == "syntax error"
                    || diagnostic.message.starts_with("missing ")),
            "{} should report a tree-sitter syntax diagnostic, got: {diagnostics:#?}",
            path.display()
        );
    }
}

fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_witcherscript::language())
        .expect("failed to load WitcherScript grammar");
    parser.parse(source, None).expect("failed to parse source")
}

fn fixture_files(kind: &str) -> Vec<PathBuf> {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(kind);
    let mut files = fs::read_dir(&fixture_dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", fixture_dir.display()))
        .map(|entry| entry.expect("failed to read fixture entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ws"))
        .collect::<Vec<_>>();

    files.sort();
    assert!(!files.is_empty(), "no {kind} fixture files found");
    files
}
