use rstest::rstest;
use witcherscript_language::builtins::{BUILTIN_ARRAY_URI, load_builtins_index};
use witcherscript_language::document::parse_document;
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{SymbolDb, WorkspaceIndex, resolve_definition};
use witcherscript_language::symbols::SymbolKind;
use witcherscript_language::test_support::TestDb;
use witcherscript_language::types::Type;

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
    workspace.update_document("file:///example.ws", &document);

    let empty = WorkspaceIndex::default();
    let definition = resolve_definition(
        "file:///example.ws",
        &document,
        &SymbolDb::new(&workspace, &empty),
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
    workspace.update_document("file:///library.ws", &library);
    workspace.update_document("file:///document.ws", &document);

    let empty = WorkspaceIndex::default();
    let definition = resolve_definition(
        "file:///document.ws",
        &document,
        &SymbolDb::new(&workspace, &empty),
        SourcePosition {
            line: 1,
            character: 15,
        },
    )
    .expect("workspace symbol should resolve");

    assert_eq!(definition.uri, "file:///library.ws");
    assert_eq!(definition.symbol.name, "CShared");
}

#[test]
fn resolves_top_level_symbol_from_base_index() {
    let base_doc = parse_document("class CGameplayEntity {}\n").expect("base should parse");
    let user_doc = parse_document("function Foo() {\n var x : CGameplayEntity;\n}\n")
        .expect("user doc should parse");
    let workspace = WorkspaceIndex::default();
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///base/gameplay.ws", &base_doc);

    // Mirrors the LSP fallthrough: try workspace first, then base index.
    let pos = SourcePosition {
        line: 1,
        character: 10,
    }; // inside "CGameplayEntity"
    let definition = resolve_definition(
        "file:///user/mod.ws",
        &user_doc,
        &SymbolDb::new(&workspace, &base),
        pos,
    )
    .expect("symbol should resolve from base index");

    assert_eq!(definition.symbol.name, "CGameplayEntity");
    assert_eq!(definition.uri, "file:///base/gameplay.ws");
}

#[test]
fn workspace_index_shadows_base_index_for_same_name() {
    let base_doc = parse_document("class CGameplayEntity {}\n").expect("base should parse");
    let workspace_doc =
        parse_document("class CGameplayEntity {}\n").expect("workspace doc should parse");
    let user_doc = parse_document("function Foo() {\n var x : CGameplayEntity;\n}\n")
        .expect("user doc should parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///user/override.ws", &workspace_doc);
    workspace.update_document("file:///user/mod.ws", &user_doc);
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///base/gameplay.ws", &base_doc);

    let pos = SourcePosition {
        line: 1,
        character: 10,
    }; // inside "CGameplayEntity"
    let definition = resolve_definition(
        "file:///user/mod.ws",
        &user_doc,
        &SymbolDb::new(&workspace, &base),
        pos,
    )
    .expect("symbol should resolve");

    assert_eq!(definition.uri, "file:///user/override.ws");
}

#[test]
fn returns_none_when_symbol_absent_from_both_indexes() {
    let user_doc =
        parse_document("function Foo() {\n var x : CUnknown;\n}\n").expect("user doc should parse");
    let workspace = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();

    let pos = SourcePosition {
        line: 1,
        character: 10,
    }; // inside "CUnknown"
    let definition = resolve_definition(
        "file:///user/mod.ws",
        &user_doc,
        &SymbolDb::new(&workspace, &base),
        pos,
    );

    assert!(definition.is_none());
}

#[test]
fn class_name_used_as_receiver_does_not_resolve() {
    // WitcherScript has no static member access; a class name used directly as
    // a receiver (CBaseClass.value) is not valid and should resolve to nothing.
    let base_doc =
        parse_document("class CBaseClass {\n var value : int;\n}\n").expect("base should parse");
    let user_doc =
        parse_document("function Foo() {\n CBaseClass.value;\n}\n").expect("user doc should parse");
    let workspace = WorkspaceIndex::default();
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///base/base_class.ws", &base_doc);

    let pos = SourcePosition {
        line: 1,
        character: 12,
    }; // inside "value" in "CBaseClass.value"
    let result = resolve_definition(
        "file:///user/mod.ws",
        &user_doc,
        &SymbolDb::new(&workspace, &base),
        pos,
    );

    assert!(
        result.is_none(),
        "static-style member access must not resolve"
    );
}

#[test]
fn builtin_array_methods_resolve_through_fixture() {
    let source = include_str!("fixtures/valid/builtin_array_usage.ws");
    let doc = parse_document(source).expect("fixture should parse");

    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///user/array_usage.ws", &doc);
    let empty = WorkspaceIndex::default();
    let builtins = load_builtins_index();
    let db = SymbolDb::new(&workspace, &empty).with_builtins(&builtins);

    let line = source
        .lines()
        .position(|l| l.contains("ints.PushBack"))
        .unwrap();
    let col = source.lines().nth(line).unwrap().find("PushBack").unwrap();
    let def = resolve_definition(
        "file:///user/array_usage.ws",
        &doc,
        &db,
        SourcePosition::new(line, col + 1),
    )
    .expect("PushBack should resolve");

    assert_eq!(def.uri, BUILTIN_ARRAY_URI);
    assert_eq!(def.symbol.name, "PushBack");
    let params = db.display_parameters_of(&def);
    assert_eq!(
        params[0].type_annotation,
        Some(Type::from_annotation("int")),
        "parameter type must be substituted"
    );
}

// `this` grants access to the receiver's private/protected members, so `this.field` must infer even when `field` is not public.
#[rstest]
#[case::private_field("private")]
#[case::protected_field("protected")]
#[case::public_field("public")]
fn resolves_inherited_method_through_field_on_this(#[case] access: &str) {
    let source = format!(
        "class CHudModule {{}}\n\
         class CHud {{ function GetHudModule(moduleName : string) : CHudModule {{}} }}\n\
         class CR4Hud extends CHud {{}}\n\
         class CR4ScriptedHud extends CR4Hud {{}}\n\
         class CExample {{\n\
         \t{access} var hud : CR4ScriptedHud;\n\
         \tfunction Use() {{\n\
         \t\tthis.hud.$0GetHudModule(\"x\");\n\
         \t}}\n\
         }}\n"
    );
    let t = TestDb::new(&source);
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).unwrap_or_else(|| {
        panic!("case {access}: inherited method should resolve through this.field")
    });
    assert_eq!(def.symbol.name, "GetHudModule", "case {access}");
}

fn assert_symbol(
    document: &witcherscript_language::document::ParsedDocument,
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
