use witcherscript_language::document::parse_document;
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{resolve_definition, SymbolDb, WorkspaceIndex};

use crate::convert::hover_markdown;

#[test]
fn formats_hover_contents_as_markdown() {
    let source = "function Make() {\n var dataObject : CScriptedFlashObject;\n dataObject = dataObject;\n}\n";
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);

    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 2,
        },
    )
    .expect("local variable should resolve");

    let markdown = hover_markdown(&definition);

    assert_eq!(
        markdown,
        "```witcherscript\nvar dataObject : CScriptedFlashObject\n```\n\nDefined in [example.ws:2](file:///example.ws#L2)"
    );
    assert!(!markdown.contains("Defined in file://"));
}

#[test]
fn formats_annotated_function_hover_with_annotation_first() {
    let source = "@wrapMethod(CR4Player)\nfunction OnSpawned(spawnData : SEntitySpawnData) {\n}\n";
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///fov.ws", &document);

    let definition = resolve_definition(
        "file:///fov.ws",
        &document,
        &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 9,
        },
    )
    .expect("function should resolve");

    let markdown = hover_markdown(&definition);

    assert_eq!(
        markdown,
        "```witcherscript\n@wrapMethod(CR4Player)\nfunction OnSpawned(spawnData: SEntitySpawnData)\n```\n\nDefined in [fov.ws:2](file:///fov.ws#L2)"
    );
}

#[test]
fn formats_parameter_hover_with_parenthesised_label() {
    let source = "function Make(spawnData : SEntitySpawnData) {\n spawnData = spawnData;\n}\n";
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);

    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 2,
        },
    )
    .expect("parameter should resolve");

    let markdown = hover_markdown(&definition);

    assert_eq!(
        markdown,
        "```witcherscript\n(parameter) spawnData : SEntitySpawnData\n```\n\nDefined in [example.ws:1](file:///example.ws#L1)"
    );
}

#[test]
fn formats_method_hover_with_owning_class_prefix() {
    let source = "class CExample {\n public function DoThing(x : int) : bool {}\n}\n";
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);

    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 17,
        },
    )
    .expect("method should resolve");

    let markdown = hover_markdown(&definition);

    assert_eq!(
        markdown,
        "```witcherscript\n(method) CExample.DoThing(x: int): bool\n```\n\nDefined in [example.ws:2](file:///example.ws#L2)"
    );
}

#[test]
fn formats_inherited_method_hover_with_superclass_name() {
    let source_a = "class A extends B {\n function Test() {\n  Inherited();\n }\n}\n";
    let source_b = "class B {\n public function Inherited() : int {}\n}\n";
    let doc_a = parse_document(source_a).expect("document should parse");
    let doc_b = parse_document(source_b).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///a.ws", &doc_a);
    workspace.update_document("file:///b.ws", &doc_b);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("inherited method should resolve");

    let text = witcherscript_language::resolve::hover_text(&definition);
    assert!(
        text.starts_with("(method) "),
        "method hover should start with '(method) '"
    );
    assert!(
        text.contains("B."),
        "method hover should include defining class"
    );
    assert!(
        text.contains("Inherited"),
        "method hover should include method name"
    );
    assert!(
        text.contains("int"),
        "method hover should include return type"
    );
}

#[test]
fn formats_field_hover_with_full_declaration() {
    let source = "class CExample {\n protected editable var ignore : bool;\n}\n";
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);

    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 25,
        },
    )
    .expect("field should resolve");

    let text = witcherscript_language::resolve::hover_text(&definition);
    assert!(
        text.starts_with("(field) "),
        "field hover should start with '(field) '"
    );
    assert!(text.contains("ignore"), "field hover should include name");
    assert!(text.contains("bool"), "field hover should include type");
}

#[test]
fn formats_class_hover_with_extends_on_single_line() {
    let source = "class Y {}\nclass X extends Y {}\n";
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);

    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 6,
        },
    )
    .expect("class should resolve");

    let text = witcherscript_language::resolve::hover_text(&definition);
    assert_eq!(
        text, "class X extends Y",
        "class hover should render the extends clause on a single line"
    );
}
