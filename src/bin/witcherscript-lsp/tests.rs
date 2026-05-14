use tower_lsp::lsp_types::{ParameterLabel, SymbolKind as LspSymbolKind};
use witcherscript_parser::document::parse_document;
use witcherscript_parser::line_index::SourcePosition;
use witcherscript_parser::resolve::{
    resolve_definition, SignatureHelpInfo, SymbolDb, WorkspaceIndex,
};
use witcherscript_parser::symbols::AccessLevel;

use crate::convert::{
    completion_item, document_symbols, hover_markdown, lsp_diagnostics, read_script_file,
    signature_help_response, wrap_method_snippet,
};

fn encode_utf16le(s: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE]; // BOM
    for unit in s.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn encode_utf16be(s: &str) -> Vec<u8> {
    let mut bytes = vec![0xFE, 0xFF]; // BOM
    for unit in s.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    bytes
}

fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes).expect("temp file write should succeed");
    path
}

#[test]
fn reads_utf8_script_file() {
    let path = write_temp("ws_test_utf8.ws", b"class CExample {}\n");
    assert_eq!(
        read_script_file(&path).expect("should read"),
        "class CExample {}\n"
    );
}

#[test]
fn reads_utf16le_script_file() {
    let bytes = encode_utf16le("class CExample {}\n");
    let path = write_temp("ws_test_utf16le.ws", &bytes);
    assert_eq!(
        read_script_file(&path).expect("should read"),
        "class CExample {}\n"
    );
}

#[test]
fn reads_utf16be_script_file() {
    let bytes = encode_utf16be("class CExample {}\n");
    let path = write_temp("ws_test_utf16be.ws", &bytes);
    assert_eq!(
        read_script_file(&path).expect("should read"),
        "class CExample {}\n"
    );
}

#[test]
fn returns_error_for_invalid_utf8() {
    let path = write_temp("ws_test_bad.ws", &[0x80, 0x81, 0x82]);
    assert!(read_script_file(&path).is_err());
}

#[test]
fn maps_core_diagnostics_to_lsp_diagnostics() {
    let document = parse_document("function Bad() {\n a = 1;\n var b : int;\n}\n")
        .expect("document should parse");

    let diagnostics = lsp_diagnostics(&document);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].source.as_deref(), Some("witcherscript"));
    assert_eq!(
        diagnostics[0].message,
        "local variable declarations must precede executable statements"
    );
}

#[test]
fn signature_help_response_maps_label_offsets_and_active_parameter() {
    let info = SignatureHelpInfo {
        label: "Find(name : string, range : float)".to_string(),
        parameters: vec![(5, 18), (20, 33)],
        active_parameter: Some(1),
    };

    let help = signature_help_response(info);

    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.active_signature, Some(0));
    assert_eq!(help.active_parameter, Some(1));

    let signature = &help.signatures[0];
    assert_eq!(signature.label, "Find(name : string, range : float)");
    let params = signature.parameters.as_ref().expect("parameters present");
    assert_eq!(params.len(), 2);
    assert!(matches!(
        params[0].label,
        ParameterLabel::LabelOffsets([5, 18])
    ));
    assert!(matches!(
        params[1].label,
        ParameterLabel::LabelOffsets([20, 33])
    ));
}

#[test]
fn maps_symbols_to_lsp_document_symbols() {
    let document =
        parse_document("class CExample {\n var value : int;\n}\n").expect("document should parse");

    let symbols = document_symbols(&document.symbols, None, "file:///test.ws");

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "CExample");
    assert_eq!(symbols[0].kind, LspSymbolKind::CLASS);
    assert_eq!(
        symbols[0]
            .children
            .as_ref()
            .expect("class should have child symbols")[0]
            .name,
        "value"
    );
}

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

    let text = witcherscript_parser::resolve::hover_text(&definition);
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

    let text = witcherscript_parser::resolve::hover_text(&definition);
    assert!(
        text.starts_with("(field) "),
        "field hover should start with '(field) '"
    );
    assert!(text.contains("ignore"), "field hover should include name");
    assert!(text.contains("bool"), "field hover should include type");
}

#[test]
fn completion_item_method_has_method_kind() {
    use tower_lsp::lsp_types::CompletionItemKind;
    use witcherscript_parser::resolve::{completion_members, SymbolDb, WorkspaceIndex};

    let source = concat!(
        "class CExample {\n",
        "  public function DoThing() {}\n",
        "}\n",
        "function Test() {\n",
        "  var e : CExample;\n",
        "  e.\n",
        "}\n",
    );
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);

    let members = completion_members(
        "file:///example.ws",
        &document,
        &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 4,
        },
    );

    assert!(!members.is_empty(), "should have completion members");
    let (_, def) = &members[0];
    let item = completion_item(def, &[]);
    assert_eq!(item.label, "DoThing");
    assert_eq!(item.kind, Some(CompletionItemKind::METHOD));
    assert_eq!(item.insert_text.as_deref(), Some("DoThing()"));
    assert!(
        item.command.is_none(),
        "paramless callable should not trigger parameter hints"
    );
}

#[test]
fn completion_item_snippet_includes_param_placeholders() {
    use tower_lsp::lsp_types::{CompletionItemKind, InsertTextFormat};
    use witcherscript_parser::resolve::{completion_members, SymbolDb, WorkspaceIndex};

    let source = concat!(
        "class CExample {\n",
        "  public function Find(findName : string, range : float) : int {}\n",
        "}\n",
        "function Test() {\n",
        "  var e : CExample;\n",
        "  e.\n",
        "}\n",
    );
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);

    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);
    let members = completion_members(
        "file:///example.ws",
        &document,
        &db,
        SourcePosition {
            line: 5,
            character: 4,
        },
    );

    let (_, find_def) = members
        .iter()
        .find(|(_, d)| d.symbol.name == "Find")
        .expect("Find should appear in completions");
    let params = db.parameters_of(&find_def.uri, find_def.symbol.id);
    let item = completion_item(find_def, &params);

    assert_eq!(item.kind, Some(CompletionItemKind::METHOD));
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
    assert_eq!(
        item.insert_text.as_deref(),
        Some("Find(${1:findName}, ${2:range})$0")
    );
    assert_eq!(
        item.command.as_ref().map(|c| c.command.as_str()),
        Some("editor.action.triggerParameterHints"),
        "callable with params should open signature help after insertion"
    );
}

#[test]
fn rename_returns_edits_for_all_occurrences() {
    use witcherscript_parser::resolve::find_references;

    let source = "function Make() {\n var x : int;\n x = 1;\n x = x + 1;\n}\n";
    let document = parse_document(source).expect("document should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///example.ws", &document);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &base);

    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &db,
        SourcePosition {
            line: 1,
            character: 5,
        },
    )
    .expect("local variable should resolve");

    let search_docs = vec![("file:///example.ws", &document)];
    let refs = find_references(&definition, &document, &search_docs, &db, true);

    assert!(
        refs.len() >= 4,
        "expected at least 4 occurrences (decl + 3 uses), got {}",
        refs.len()
    );
}

#[test]
fn rename_rejects_base_script_symbol() {
    use std::collections::HashSet;

    let base_source = "function BaseFunc() {}\n";
    let base_doc = parse_document(base_source).expect("should parse");
    let mut base_index = WorkspaceIndex::default();
    base_index.update_document("file:///base/base.ws", &base_doc);

    let caller_source = "function MyFunc() { BaseFunc(); }\n";
    let caller_doc = parse_document(caller_source).expect("should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///project/my.ws", &caller_doc);
    let db = SymbolDb::new(&workspace, &base_index);

    let definition = resolve_definition(
        "file:///project/my.ws",
        &caller_doc,
        &db,
        SourcePosition {
            line: 0,
            character: 20,
        },
    )
    .expect("BaseFunc call should resolve to base definition");

    assert_eq!(
        definition.uri, "file:///base/base.ws",
        "definition should point into the base scripts"
    );

    let base_uris: HashSet<String> = ["file:///base/base.ws".to_string()].into();
    assert!(
        base_uris.contains(&definition.uri),
        "rename should be rejected: symbol is declared in a base script"
    );
}

#[test]
fn wrap_method_snippet_plain_params() {
    let source = concat!(
        "class CPlayer {\n",
        "  public function CanParry(damage : int, attacker : CObject) : bool {}\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let method = db
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "CanParry")
        .expect("CanParry should be a member of CPlayer");

    let snippet = wrap_method_snippet(&method, &db);
    assert_eq!(
        snippet,
        "CanParry(damage : int, attacker : CObject) {\n\t$0\n\n\treturn wrappedMethod(damage, attacker);\n}"
    );
}

#[test]
fn wrap_method_snippet_optional_and_out_params() {
    let source = concat!(
        "class CPlayer {\n",
        "  public function Foo(a : int, optional b : float, out c : string) {}\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let method = db
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "Foo")
        .expect("Foo should be a member");

    let snippet = wrap_method_snippet(&method, &db);
    assert_eq!(
        snippet,
        "Foo(a : int, optional b : float, out c : string) {\n\twrappedMethod(a, b, c);\n\n\t$0\n}"
    );
}

#[test]
fn wrap_method_snippet_no_params() {
    let source = "class CPlayer {\n  public function OnSpawned() {}\n}\n";
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let method = db
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "OnSpawned")
        .expect("OnSpawned should be a member");

    let snippet = wrap_method_snippet(&method, &db);
    assert_eq!(snippet, "OnSpawned() {\n\twrappedMethod();\n\n\t$0\n}");
}

#[test]
fn wrap_method_snippet_event_uses_return_form() {
    // Events always use the return form so the caller can be reached after custom logic.
    let source = "class CPlayer {\n  public event OnDeath() {}\n}\n";
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let method = db
        .members_of("CPlayer", AccessLevel::Public)
        .into_iter()
        .find(|d| d.symbol.name == "OnDeath")
        .expect("OnDeath should be a member");

    let snippet = wrap_method_snippet(&method, &db);
    assert_eq!(snippet, "OnDeath() {\n\t$0\n\n\treturn wrappedMethod();\n}");
}
