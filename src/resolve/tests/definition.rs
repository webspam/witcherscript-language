use super::super::{resolve_all_definitions, resolve_definition};
use super::{make_doc, make_index, SymbolDb, WorkspaceIndex};
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

#[test]
fn resolves_definition_site_of_top_level_function() {
    let document = make_doc("function Foo() {}\n");
    let index = WorkspaceIndex::default();

    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 9,
        },
    )
    .expect("definition should resolve from its own definition site");

    assert_eq!(definition.symbol.name, "Foo");
    assert_eq!(definition.symbol.kind, SymbolKind::Function);
}

#[test]
fn resolves_definition_at_word_boundary() {
    // "function Foo() {}\n"
    //           0123
    // character 12 is just past the final 'o' of "Foo"
    let document = make_doc("function Foo() {}\n");
    let index = WorkspaceIndex::default();

    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 12,
        },
    )
    .expect("definition should resolve when caret is one past the last letter");

    assert_eq!(definition.symbol.name, "Foo");
}

#[test]
fn resolves_definition_site_of_class_method() {
    let document = make_doc("class CExample {\n function Bar() {}\n}\n");
    let index = WorkspaceIndex::default();

    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 10,
        },
    )
    .expect("definition should resolve from its own definition site");

    assert_eq!(definition.symbol.name, "Bar");
    assert_eq!(definition.symbol.kind, SymbolKind::Method);
}

#[test]
fn resolves_definition_site_of_enum_variant() {
    let document = make_doc("enum EFoo {\n VALUE_A = 0\n}\n");
    let index = WorkspaceIndex::default();

    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 1,
        },
    )
    .expect("definition should resolve from enum variant definition site");

    assert_eq!(definition.symbol.name, "VALUE_A");
    assert_eq!(definition.symbol.kind, SymbolKind::EnumVariant);
}

#[test]
fn resolves_enum_variant_reference_in_expression() {
    let document =
        make_doc("enum EColor { ERed = 0, EBlue = 1 }\nfunction F() { var c : EColor = ERed; }\n");
    let index = make_index("file:///test.ws", &document);

    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 33,
        },
    )
    .expect("enum variant reference in expression should resolve");

    assert_eq!(definition.symbol.name, "ERed");
    assert_eq!(definition.symbol.kind, SymbolKind::EnumVariant);
    assert_eq!(definition.symbol.selection_range.start.line, 0);
}

#[test]
fn resolves_enum_variant_defined_in_other_document() {
    let enum_doc = make_doc("enum EColor { ERed = 0 }\n");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///enums.ws", &enum_doc);

    let user_doc = make_doc("function F() { var c : EColor; c = ERed; }\n");
    let definition = resolve_definition(
        "file:///user.ws",
        &user_doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 36,
        },
    )
    .expect("cross-document enum variant reference should resolve");

    assert_eq!(definition.uri, "file:///enums.ws");
    assert_eq!(definition.symbol.name, "ERed");
    assert_eq!(definition.symbol.kind, SymbolKind::EnumVariant);
}

#[test]
fn resolves_receiver_variable_itself_in_member_access() {
    let source = concat!(
        "class Example {\n",
        "  function Test() {\n",
        "    var unrelated : UnrelatedClass;\n",
        "    unrelated.Initialize();\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let index = WorkspaceIndex::default();

    // cursor on 'unrelated' in 'unrelated.Initialize()' — line 3, col 4
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 3,
            character: 4,
        },
    )
    .expect("receiver variable should resolve to its declaration");

    assert_eq!(definition.symbol.name, "unrelated");
    assert_eq!(definition.symbol.kind, SymbolKind::Variable);
}

#[test]
fn unknown_receiver_dot_method_resolves_to_nothing() {
    let source = concat!(
        "class Example {\n",
        "  public function Initialize() {\n",
        "    typo.Initialize();\n",
        "  }\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let result = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 9,
        },
    );
    assert!(
        result.is_none(),
        "unknown receiver must not fall back to current class"
    );
}

#[test]
fn resolves_variable_dot_method_to_declared_type_not_current_class() {
    // Regression: unrelated.Initialize() inside Example should resolve to
    // UnrelatedClass.Initialize, not Example.Initialize.
    let source = concat!(
        "class Example {\n",
        "  public function Initialize() {\n",
        "    var unrelated : UnrelatedClass = new UnrelatedClass in this;\n",
        "    unrelated.Initialize();\n",
        "  }\n",
        "}\n",
        "class UnrelatedClass {\n",
        "  public function Initialize() {}\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // line 3, col 14 — "Initialize" after "unrelated."
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 3,
            character: 14,
        },
    )
    .expect("should resolve to UnrelatedClass.Initialize");

    assert_eq!(definition.symbol.name, "Initialize");
    let container_id = definition
        .symbol
        .container
        .expect("method should have a container");
    let container = doc
        .symbols
        .by_id(container_id)
        .expect("container should exist");
    assert_eq!(container.name, "UnrelatedClass");
}

#[test]
fn resolves_parameter_before_top_level() {
    let document = make_doc("function value() {}\nfunction test(value : int) {\n value = 1;\n}\n");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document);

    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 1,
        },
    )
    .expect("definition should resolve");

    assert_eq!(definition.symbol.kind, SymbolKind::Parameter);
}

#[test]
fn goto_def_on_var_initializer_ident_does_not_self_resolve() {
    type Case = (
        &'static str,
        &'static str,
        SourcePosition,
        Option<(&'static str, SymbolKind, u32)>,
    );
    let cases: &[Case] = &[
        (
            "initializer references a prior local — resolves to prior decl",
            concat!(
                "function Test() {\n",
                "  var source : int;\n",
                "  var x : int = source;\n",
                "}\n",
            ),
            SourcePosition {
                line: 2,
                character: 16,
            },
            Some(("source", SymbolKind::Variable, 1)),
        ),
        (
            "initializer references a parameter — resolves to parameter",
            "function Test(p : int) {\n  var x : int = p;\n}\n",
            SourcePosition {
                line: 1,
                character: 16,
            },
            Some(("p", SymbolKind::Parameter, 0)),
        ),
        (
            "initializer is undeclared — no fake local self-resolves",
            "function Test() {\n  var x : int = ghost;\n}\n",
            SourcePosition {
                line: 1,
                character: 16,
            },
            None,
        ),
    ];

    for (msg, source, position, expected) in cases.iter().copied() {
        let doc = make_doc(source);
        let mut index = WorkspaceIndex::default();
        index.update_document("file:///test.ws", &doc);
        let empty = WorkspaceIndex::default();
        let db = SymbolDb::new(&index, &empty);

        match (
            resolve_definition("file:///test.ws", &doc, &db, position),
            expected,
        ) {
            (Some(def), Some((name, kind, decl_line))) => {
                assert_eq!(def.symbol.name, name, "{msg}: name");
                assert_eq!(def.symbol.kind, kind, "{msg}: kind");
                assert_eq!(
                    def.symbol.selection_range.start.line, decl_line,
                    "{msg}: should point at declaration site, not the initializer use"
                );
            }
            (None, None) => {}
            (Some(def), None) => panic!(
                "{msg}: expected no resolution, got `{}` at line {}",
                def.symbol.name, def.symbol.selection_range.start.line
            ),
            (None, Some(_)) => panic!("{msg}: expected a resolution, got None"),
        }
    }
}

// --- resolve_all_definitions: multi-declaration go-to-definition ---

fn index_docs(docs: &[(&str, &ParsedDocument)]) -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();
    for (uri, doc) in docs {
        index.update_document(*uri, doc);
    }
    index
}

#[test]
fn add_method_resolves_from_call_site() {
    let base = make_doc("class CPlayer {\n  public function Heal() {}\n}\n");
    let mod_a = make_doc("@addMethod(CPlayer)\nfunction Boost() {}\n");
    let caller = make_doc("function Caller() {\n  var p : CPlayer;\n  p.Boost();\n}\n");
    let index = index_docs(&[
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let definition = resolve_definition(
        "file:///caller.ws",
        &caller,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    )
    .expect("a call to an @addMethod method must resolve");
    assert_eq!(definition.symbol.name, "Boost");
    assert_eq!(definition.uri, "file:///a.ws");
}

#[test]
fn goto_def_from_call_site_returns_class_body_and_wrap() {
    let base = make_doc("class CPlayer {\n  public function OnSpawned() {}\n}\n");
    let mod_a = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {}\n");
    let caller = make_doc("function Caller() {\n  var p : CPlayer;\n  p.OnSpawned();\n}\n");
    let index = index_docs(&[
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let defs = resolve_all_definitions(
        "file:///caller.ws",
        &caller,
        &db,
        SourcePosition {
            line: 2,
            character: 6,
        },
    );
    assert_eq!(
        defs.len(),
        2,
        "class-body declaration plus the wrap declaration"
    );
    assert_eq!(
        defs[0].uri, "file:///base.ws",
        "class-body declaration first"
    );
    assert!(defs.iter().any(|d| d.uri == "file:///a.ws"));
}

#[test]
fn goto_def_from_wrap_function_name_returns_all() {
    let base = make_doc("class CPlayer {\n  public function OnSpawned() {}\n}\n");
    let mod_a = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///a.ws", &mod_a)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let defs = resolve_all_definitions(
        "file:///a.ws",
        &mod_a,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    );
    assert_eq!(defs.len(), 2);
    assert!(defs.iter().any(|d| d.uri == "file:///base.ws"));
    assert!(defs.iter().any(|d| d.uri == "file:///a.ws"));
}

#[test]
fn goto_def_single_when_no_wrap() {
    let doc = make_doc("class CExample {\n  function Bar() {}\n}\n");
    let index = index_docs(&[("file:///test.ws", &doc)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let defs = resolve_all_definitions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 11,
        },
    );
    assert_eq!(defs.len(), 1, "a plain method has exactly one declaration");
    assert_eq!(defs[0].symbol.name, "Bar");
}

#[test]
fn goto_def_multiple_wraps_same_method() {
    let base = make_doc("class CPlayer {\n  public function OnSpawned() {}\n}\n");
    let mod_a = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {}\n");
    let mod_b = make_doc("@wrapMethod(CPlayer)\nfunction OnSpawned() {}\n");
    let index = index_docs(&[
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///b.ws", &mod_b),
    ]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let defs = resolve_all_definitions(
        "file:///a.ws",
        &mod_a,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    );
    assert_eq!(defs.len(), 3, "class-body plus two wrap declarations");
}

#[test]
fn goto_def_replace_method_included() {
    let base = make_doc("class CPlayer {\n  public function OnSpawned() {}\n}\n");
    let mod_a = make_doc("@replaceMethod(CPlayer)\nfunction OnSpawned() {}\n");
    let index = index_docs(&[("file:///base.ws", &base), ("file:///a.ws", &mod_a)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let defs = resolve_all_definitions(
        "file:///a.ws",
        &mod_a,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    );
    assert_eq!(defs.len(), 2);
    assert!(defs.iter().any(|d| d.uri == "file:///a.ws"));
}

#[test]
fn goto_def_wrap_unknown_class_returns_just_self() {
    let mod_a = make_doc("@wrapMethod(CGhost)\nfunction Haunt() {}\n");
    let index = index_docs(&[("file:///a.ws", &mod_a)]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let defs = resolve_all_definitions(
        "file:///a.ws",
        &mod_a,
        &db,
        SourcePosition {
            line: 1,
            character: 11,
        },
    );
    assert_eq!(
        defs.len(),
        1,
        "no real class — only the wrap function itself"
    );
    assert_eq!(defs[0].uri, "file:///a.ws");
}

#[test]
fn goto_def_add_method_no_class_body_returns_annotated() {
    let base = make_doc("class CPlayer {\n  public function Heal() {}\n}\n");
    let mod_a = make_doc("@addMethod(CPlayer)\nfunction Boost() {}\n");
    let caller = make_doc("function Caller() {\n  var p : CPlayer;\n  p.Boost();\n}\n");
    let index = index_docs(&[
        ("file:///base.ws", &base),
        ("file:///a.ws", &mod_a),
        ("file:///caller.ws", &caller),
    ]);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &empty);

    let defs = resolve_all_definitions(
        "file:///caller.ws",
        &caller,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    );
    assert_eq!(defs.len(), 1, "@addMethod has no class-body counterpart");
    assert_eq!(defs[0].uri, "file:///a.ws");
}
