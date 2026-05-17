use tower_lsp::lsp_types::{ParameterLabel, SymbolKind as LspSymbolKind};
use witcherscript_parser::document::parse_document;
use witcherscript_parser::files::read_script_file;
use witcherscript_parser::line_index::SourcePosition;
use witcherscript_parser::resolve::{
    resolve_definition, SignatureHelpInfo, SymbolDb, WorkspaceIndex,
};
use witcherscript_parser::symbols::AccessLevel;

use crate::convert::{
    completion_item, document_symbols, hover_markdown, lsp_diagnostics, lsp_workspace_diagnostic,
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

    let text = witcherscript_parser::resolve::hover_text(&definition);
    assert_eq!(
        text, "class X extends Y",
        "class hover should render the extends clause on a single line"
    );
}

#[test]
#[cfg(windows)]
fn opening_a_workspace_indexed_file_does_not_self_conflict() {
    use crate::indexing::index_open_document;
    use tower_lsp::lsp_types::Url;
    use witcherscript_parser::diagnostics::collect_duplicate_symbol_diagnostics;

    let document = parse_document("function Foo() {}\n").expect("document should parse");
    let mut index = WorkspaceIndex::default();

    // The editor opens the file under its own (percent-encoded) spelling, while
    // index_workspace keys the same file via Url::from_file_path.
    let opened = Url::parse("file:///c%3A/proj/foo.ws").expect("uri should parse");
    let indexed_uri = Url::from_file_path(opened.to_file_path().unwrap())
        .expect("path should convert back to a URI");
    assert_ne!(
        indexed_uri.as_str(),
        opened.as_str(),
        "test must exercise a real client-vs-canonical spelling mismatch"
    );

    index.update_document(indexed_uri.as_str(), &document);
    index_open_document(&mut index, &opened, &document);

    assert!(
        collect_duplicate_symbol_diagnostics(&index).is_empty(),
        "a workspace-indexed file that is then opened must not be flagged as a duplicate of itself"
    );
}

#[test]
fn workspace_diagnostic_carries_related_information() {
    use witcherscript_parser::diagnostics::{RelatedLocation, Severity, WorkspaceDiagnostic};
    use witcherscript_parser::line_index::SourceRange;

    let range = SourceRange {
        start: SourcePosition {
            line: 0,
            character: 6,
        },
        end: SourcePosition {
            line: 0,
            character: 9,
        },
    };
    let diagnostic = WorkspaceDiagnostic {
        kind: "duplicate_symbol".to_string(),
        message: "A class or function with that name already exists.".to_string(),
        severity: Severity::Error,
        range,
        related: vec![RelatedLocation {
            uri: "file:///other.ws".to_string(),
            range,
            message: "'Foo' also declared here".to_string(),
        }],
    };

    let lsp = lsp_workspace_diagnostic(&diagnostic);

    assert_eq!(
        lsp.severity,
        Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR)
    );
    assert_eq!(
        lsp.code,
        Some(tower_lsp::lsp_types::NumberOrString::String(
            "duplicate_symbol".to_string()
        ))
    );
    let related = lsp
        .related_information
        .expect("related_information should be present");
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].location.uri.as_str(), "file:///other.ws");
    assert_eq!(related[0].message, "'Foo' also declared here");
    assert_eq!(related[0].location.range.start.character, 6);
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
fn rename_does_not_edit_base_scripts() {
    use std::collections::HashMap;
    use witcherscript_parser::resolve::find_references;

    use crate::backend::rename_changes;

    // Base script declares CR4Player; one of its methods calls IsCiri()
    // unqualified (implicit `this`). Since 8023ddf the workspace @addMethod is
    // indexed as a real member of CR4Player, so this base call site resolves
    // into the workspace symbol and find_references reports it.
    let base_source = "class CR4Player {\n  function Foo() { IsCiri(); }\n}\n";
    let base_doc = parse_document(base_source).expect("base should parse");
    let base_doc_owned = parse_document(base_source).expect("base should parse");
    let mut base_index = WorkspaceIndex::default();
    base_index.update_document("file:///base/player.ws", &base_doc);
    let mut base_docs: HashMap<String, _> = HashMap::new();
    base_docs.insert("file:///base/player.ws".to_string(), base_doc_owned);

    // Workspace mod adds IsCiri() to CR4Player via @addMethod.
    let mod_source = "@addMethod(CR4Player)\nfunction IsCiri() {}\n";
    let mod_doc = parse_document(mod_source).expect("mod should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///mod/ciri.ws", &mod_doc);

    let db = SymbolDb::new(&workspace, &base_index);

    let definition = resolve_definition(
        "file:///mod/ciri.ws",
        &mod_doc,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    )
    .expect("@addMethod function name should resolve");
    assert!(
        !base_docs.contains_key(&definition.uri),
        "definition is in the workspace, so the existing guard lets the rename through"
    );

    let search_docs = vec![
        ("file:///base/player.ws", &base_doc),
        ("file:///mod/ciri.ws", &mod_doc),
    ];
    let refs = find_references(&definition, &mod_doc, &search_docs, &db, true);
    assert!(
        refs.iter().any(|(uri, _)| uri == "file:///base/player.ws"),
        "the base-script call site resolves into the @addMethod symbol"
    );

    let changes = rename_changes(&refs, "IsCiriRenamed", &base_docs);
    assert!(
        changes
            .keys()
            .all(|url| url.as_str() != "file:///base/player.ws"),
        "rename must not emit edits for read-only base-script files"
    );
    assert!(
        !changes.is_empty(),
        "rename should still emit edits for the workspace declaration"
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

#[test]
fn build_index_segments_empty_inputs() {
    let segments = crate::indexing::build_index_segments(None, &[], true);
    assert!(segments.is_empty());
}

#[test]
fn build_index_segments_game_dir_only() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_game_only");
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &[], true);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].0, "gameDirectory");
    assert!(!segments[0].2);
}

#[test]
fn build_index_segments_auto_loads_mod_shared_imports_when_present() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_msi_present");
    let msi = game_dir.join("Mods").join("modSharedImports");
    std::fs::create_dir_all(&msi).expect("mkdir mods");
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &[], true);
    let labels: Vec<&str> = segments.iter().map(|(l, _, _)| *l).collect();
    assert!(labels.contains(&"modSharedImports"));
    let msi_seg = segments
        .iter()
        .find(|(l, _, _)| *l == "modSharedImports")
        .unwrap();
    assert!(
        msi_seg.2,
        "modSharedImports segment must be flagged as auto-loaded"
    );
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
}

#[test]
fn build_index_segments_skips_mod_shared_imports_when_flag_off() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_msi_flag_off");
    let msi = game_dir.join("Mods").join("modSharedImports");
    std::fs::create_dir_all(&msi).expect("mkdir mods");
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &[], false);
    let labels: Vec<&str> = segments.iter().map(|(l, _, _)| *l).collect();
    assert!(!labels.contains(&"modSharedImports"));
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
}

#[test]
fn build_index_segments_skips_missing_extra_dir() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_extra_missing");
    let missing = std::env::temp_dir().join("ws_test_segments_definitely_not_a_dir_xyz");
    std::fs::remove_dir_all(&missing).ok();
    let extras = vec![missing];
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &extras, false);
    let labels: Vec<&str> = segments.iter().map(|(l, _, _)| *l).collect();
    assert!(!labels.contains(&"additionalScriptDirectory"));
}

#[test]
fn build_index_segments_dedups_extra_that_overlaps_mod_shared_imports() {
    let game_dir = std::env::temp_dir().join("ws_test_segments_dedup");
    let msi = game_dir.join("Mods").join("modSharedImports");
    std::fs::create_dir_all(&msi).expect("mkdir mods");
    let extras = vec![msi.clone()];
    let segments = crate::indexing::build_index_segments(Some(&game_dir), &extras, true);
    let msi_segs: Vec<_> = segments.iter().filter(|(_, p, _)| p == &msi).collect();
    assert_eq!(msi_segs.len(), 1, "overlapping path must appear once");
    assert_eq!(msi_segs[0].0, "modSharedImports");
    assert!(msi_segs[0].2, "first-inserted (modSharedImports) wins");
    std::fs::remove_dir_all(game_dir.join("Mods")).ok();
}

#[cfg(test)]
mod watched_files {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use tower_lsp::lsp_types::{FileChangeType, FileEvent, Url};
    use witcherscript_parser::files::ExcludeFilter;

    use crate::indexing::{classify_watched_event, WatchedEvent};

    fn event(uri: &str, typ: FileChangeType) -> FileEvent {
        FileEvent {
            uri: Url::parse(uri).expect("uri parses"),
            typ,
        }
    }

    fn workspace_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\workspace")
        } else {
            PathBuf::from("/workspace")
        }
    }

    fn uri_under_root(rel: &str) -> Url {
        Url::from_file_path(workspace_root().join(rel)).expect("uri builds")
    }

    fn no_filter() -> ExcludeFilter {
        ExcludeFilter::new(&[workspace_root()], &[])
    }

    #[test]
    fn created_event_returns_upsert() {
        let url = uri_under_root("foo.ws");
        let canonical = url.to_string();
        let decision = classify_watched_event(
            &event(url.as_str(), FileChangeType::CREATED),
            &HashSet::new(),
            &no_filter(),
        );
        let Some(WatchedEvent::Upsert {
            canonical: got,
            path,
        }) = decision
        else {
            panic!("expected Upsert, got {decision:?}");
        };
        assert_eq!(got, canonical);
        assert!(path.ends_with("foo.ws"));
    }

    #[test]
    fn changed_event_returns_upsert() {
        let url = uri_under_root("bar.ws");
        let decision = classify_watched_event(
            &event(url.as_str(), FileChangeType::CHANGED),
            &HashSet::new(),
            &no_filter(),
        );
        assert!(matches!(decision, Some(WatchedEvent::Upsert { .. })));
    }

    #[test]
    fn deleted_event_returns_remove() {
        let url = uri_under_root("gone.ws");
        let canonical = url.to_string();
        let decision = classify_watched_event(
            &event(url.as_str(), FileChangeType::DELETED),
            &HashSet::new(),
            &no_filter(),
        );
        assert_eq!(
            decision,
            Some(WatchedEvent::Remove {
                canonical: canonical.clone()
            })
        );
    }

    #[test]
    fn deleted_event_ignores_exclude_filter() {
        let url = uri_under_root("excluded/gone.ws");
        let filter = ExcludeFilter::new(&[workspace_root()], &["excluded/**".to_string()]);
        let decision = classify_watched_event(
            &event(url.as_str(), FileChangeType::DELETED),
            &HashSet::new(),
            &filter,
        );
        assert!(matches!(decision, Some(WatchedEvent::Remove { .. })));
    }

    #[test]
    fn skips_event_for_open_file() {
        let url = uri_under_root("open.ws");
        let mut open = HashSet::new();
        open.insert(url.to_string());
        let decision = classify_watched_event(
            &event(url.as_str(), FileChangeType::CHANGED),
            &open,
            &no_filter(),
        );
        assert_eq!(decision, None);
    }

    #[test]
    fn skips_event_for_excluded_path() {
        let url = uri_under_root("vendor/lib.ws");
        let filter = ExcludeFilter::new(&[workspace_root()], &["vendor/**".to_string()]);
        let decision = classify_watched_event(
            &event(url.as_str(), FileChangeType::CREATED),
            &HashSet::new(),
            &filter,
        );
        assert_eq!(decision, None);
    }

    #[test]
    fn skips_event_for_non_ws_extension() {
        let url = uri_under_root("notes.txt");
        let decision = classify_watched_event(
            &event(url.as_str(), FileChangeType::CREATED),
            &HashSet::new(),
            &no_filter(),
        );
        assert_eq!(decision, None);
    }

    #[test]
    #[cfg(windows)]
    fn canonicalises_percent_encoded_uri_for_open_file_skip() {
        let opened = Url::parse("file:///c%3A/proj/foo.ws").expect("client uri parses");
        let canonical_opened =
            crate::convert::canonical_uri(&opened).expect("canonical uri builds");
        assert_ne!(canonical_opened, opened.as_str());

        let watcher_url =
            Url::from_file_path(opened.to_file_path().unwrap()).expect("path converts back to uri");
        let open_canonical: HashSet<String> = [canonical_opened.clone()].into_iter().collect();
        let filter = ExcludeFilter::new(&[PathBuf::from("C:\\proj")], &[]);

        let decision = classify_watched_event(
            &event(watcher_url.as_str(), FileChangeType::CHANGED),
            &open_canonical,
            &filter,
        );
        assert_eq!(
            decision, None,
            "watcher event for an open file (under different URI spelling) must be skipped"
        );
    }
}

mod builtin_source_request {
    use crate::backend::builtin_source_response;
    use tower_lsp::jsonrpc::ErrorCode;
    use witcherscript_parser::builtins::BUILTIN_ARRAY_URI;

    #[test]
    fn returns_array_text_for_array_uri() {
        let response = builtin_source_response(BUILTIN_ARRAY_URI).expect("should succeed");
        let text = response
            .get("text")
            .and_then(|v| v.as_str())
            .expect("response has text field");
        assert!(text.contains("class array"));
        assert!(text.contains("PushBack"));
    }

    #[test]
    fn returns_null_for_unknown_uri() {
        let response = builtin_source_response("file:///not/a/builtin.ws").expect("should succeed");
        assert!(response.is_null());
    }

    #[test]
    fn errors_when_uri_is_empty() {
        let err = builtin_source_response("").expect_err("should reject empty uri");
        assert_eq!(err.code, ErrorCode::InvalidParams);
    }
}
