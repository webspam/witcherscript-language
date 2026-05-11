use witcherscript_parser::document::parse_document;
use witcherscript_parser::line_index::SourcePosition;
use witcherscript_parser::resolve::{resolve_definition, WorkspaceIndex};
use witcherscript_parser::symbols::SymbolKind;

#[test]
fn extracts_outline_symbols_from_mod_annotations_fixture() {
    let source = include_str!("fixtures/valid/mod_annotations_and_defaults.ws");
    let document = parse_document(source).expect("fixture should parse");

    assert_symbol(&document, "EParserFixtureKind", SymbolKind::Enum);
    assert_symbol(
        &document,
        "SParserFixtureOriginalValues",
        SymbolKind::Struct,
    );
    assert_symbol(&document, "CParserFixtureParams", SymbolKind::Class);
    assert_symbol(&document, "ParserFixtureWrapped", SymbolKind::Function);
    assert_symbol(&document, "ParserFixtureTimer", SymbolKind::Function);

    let wrapped = document
        .symbols
        .all()
        .iter()
        .find(|symbol| symbol.name == "ParserFixtureWrapped")
        .expect("wrapped method symbol should exist");
    assert_eq!(wrapped.annotations[0].name, "wrapMethod");
    assert_eq!(
        wrapped.annotations[0].argument.as_deref(),
        Some("CR4Player")
    );
}

#[test]
fn resolves_same_file_member_access_on_this() {
    let source =
        "class CExample {\n var value : int;\n function Set() {\n  this.value = 1;\n }\n}\n";
    let document = parse_document(source).expect("source should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document.symbols);

    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &workspace,
        SourcePosition {
            line: 3,
            character: 7,
        },
    )
    .expect("member should resolve");

    assert_eq!(definition.symbol.name, "value");
    assert_eq!(definition.symbol.kind, SymbolKind::Field);
}

#[test]
fn resolves_workspace_top_level_symbols() {
    let library = parse_document("class CShared {}\n").expect("library should parse");
    let document = parse_document("function Make() {\n var shared : CShared;\n}\n")
        .expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///library.ws", &library.symbols);
    workspace.update_document("file:///document.ws", &document.symbols);

    let definition = resolve_definition(
        "file:///document.ws",
        &document,
        &workspace,
        SourcePosition {
            line: 1,
            character: 15,
        },
    )
    .expect("workspace symbol should resolve");

    assert_eq!(definition.uri, "file:///library.ws");
    assert_eq!(definition.symbol.name, "CShared");
}

fn assert_symbol(
    document: &witcherscript_parser::document::ParsedDocument,
    name: &str,
    kind: SymbolKind,
) {
    assert!(
        document
            .symbols
            .all()
            .iter()
            .any(|symbol| symbol.name == name && symbol.kind == kind),
        "expected {kind:?} symbol named {name}"
    );
}
