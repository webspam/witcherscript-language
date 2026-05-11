use crate::document::parse_document;
use crate::line_index::SourcePosition;

use crate::script_env::ScriptEnvironment;
use crate::symbols::AccessLevel;

use super::{resolve_definition, SymbolDb, WorkspaceIndex};

#[test]
fn resolves_definition_site_of_top_level_function() {
    let document = parse_document("function Foo() {}\n").expect("parse should succeed");
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
    assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Function);
}

#[test]
fn resolves_definition_at_word_boundary() {
    // "function Foo() {}\n"
    //           0123
    // character 12 is just past the final 'o' of "Foo"
    let document = parse_document("function Foo() {}\n").expect("parse should succeed");
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
    let document =
        parse_document("class CExample {\n function Bar() {}\n}\n").expect("parse should succeed");
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
    assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Method);
}

#[test]
fn resolves_definition_site_of_enum_variant() {
    let document = parse_document("enum EFoo {\n VALUE_A = 0\n}\n").expect("parse should succeed");
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
    assert_eq!(
        definition.symbol.kind,
        crate::symbols::SymbolKind::EnumVariant
    );
}

#[test]
fn finds_references_to_top_level_function() {
    let source = "function Foo() {}\nfunction Bar() {\n Foo();\n Foo();\n}\n";
    let document = parse_document(source).expect("parse should succeed");
    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&WorkspaceIndex::default(), &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 9,
        },
    )
    .expect("definition should resolve");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document.symbols);

    let refs = super::find_references(
        &definition,
        &document,
        &[("file:///test.ws", &document)],
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        false,
    );
    assert_eq!(refs.len(), 2, "two call sites expected");
}

#[test]
fn find_references_respects_include_declaration() {
    let source = "function Foo() {}\nfunction Bar() {\n Foo();\n}\n";
    let document = parse_document(source).expect("parse should succeed");
    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&WorkspaceIndex::default(), &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 9,
        },
    )
    .expect("definition should resolve");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document.symbols);

    let with_decl = super::find_references(
        &definition,
        &document,
        &[("file:///test.ws", &document)],
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        true,
    );
    let without_decl = super::find_references(
        &definition,
        &document,
        &[("file:///test.ws", &document)],
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        false,
    );
    assert_eq!(with_decl.len(), 2);
    assert_eq!(without_decl.len(), 1);
}

#[test]
fn finds_references_to_local_variable_within_function_scope() {
    let source =
        "function Outer() {\n var x : int;\n x = 1;\n}\nfunction Other() {\n var x : int;\n}\n";
    let document = parse_document(source).expect("parse should succeed");
    let definition = resolve_definition(
        "file:///test.ws",
        &document,
        &SymbolDb::new(&WorkspaceIndex::default(), &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 1,
        },
    )
    .expect("local variable should resolve");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document.symbols);

    let refs = super::find_references(
        &definition,
        &document,
        &[("file:///test.ws", &document)],
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        true,
    );
    // Should find x in Outer() only: the declaration and the assignment
    assert_eq!(refs.len(), 2, "x in Other() should not be included");
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
    let doc = parse_document(source).expect("parse should succeed");
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
    assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Variable);
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
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc.symbols);

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
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc.symbols);

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
    // The definition must live inside UnrelatedClass, not Example
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
fn resolves_this_keyword_to_current_class() {
    let source = "class MyClass {\n function Test() {\n  this.Foo();\n }\n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc.symbols);

    // cursor on 'this' (line 2, col 3)
    let definition = resolve_definition(
        "file:///a.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("this keyword should navigate to current class");

    assert_eq!(definition.symbol.name, "MyClass");
    assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Class);
}

#[test]
fn resolves_super_keyword_to_parent_class() {
    let source_a = "class A extends B {\n function Test() {\n  super.Method();\n }\n}\n";
    let source_b = "class B {\n function Method() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);

    // cursor on 'super' (line 2, col 3)
    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("super keyword should navigate to parent class");

    assert_eq!(definition.symbol.name, "B");
    assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Class);
}

#[test]
fn resolves_inherited_method_via_workspace() {
    let source_a = "class A extends B {\n function Test() {\n  Inherited();\n }\n}\n";
    let source_b = "class B {\n function Inherited() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("inherited method should resolve");

    assert_eq!(definition.symbol.name, "Inherited");
    assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Method);
}

#[test]
fn class_without_explicit_extends_defaults_to_cobject() {
    let doc = parse_document("class A {}").expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc.symbols);
    // CObject is not in the index; find_member must terminate without looping.
    assert!(index
        .find_member("A", "someMethod", AccessLevel::Public)
        .is_none());
}

#[test]
fn resolves_inherited_method_unqualified_inside_subclass() {
    let source_a = "class A extends B {\n function Test() {\n  Inherited();\n }\n}\n";
    let source_b = "class B {\n function Inherited() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 3,
        },
    )
    .expect("unqualified inherited method should resolve inside subclass body");

    assert_eq!(definition.symbol.name, "Inherited");
}

#[test]
fn resolves_this_dot_inherited_method() {
    let source_a = "class A extends B {\n function Test() {\n  this.Inherited();\n }\n}\n";
    let source_b = "class B {\n function Inherited() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 8,
        },
    )
    .expect("this.Inherited() should resolve to superclass method");

    assert_eq!(definition.symbol.name, "Inherited");
}

#[test]
fn resolves_method_on_class_field_receiver() {
    let source = concat!(
        "class Foo {\n",
        "  private var gConfig : CInGameConfigWrapper;\n",
        "  function someFunc() {\n",
        "    gConfig.GetSpecialConfig();\n",
        "  }\n",
        "}\n",
        "class CInGameConfigWrapper {\n",
        "  function GetSpecialConfig() {}\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc.symbols);

    // cursor on 'GetSpecialConfig' (line 3, col 12)
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 3,
            character: 12,
        },
    )
    .expect("method on class field should resolve");

    assert_eq!(definition.symbol.name, "GetSpecialConfig");
}

#[test]
fn resolves_parameter_before_top_level() {
    let document =
        parse_document("function value() {}\nfunction test(value : int) {\n value = 1;\n}\n")
            .expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &document.symbols);

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

    assert_eq!(
        definition.symbol.kind,
        crate::symbols::SymbolKind::Parameter
    );
}

#[test]
fn private_method_not_visible_in_subclass() {
    let source_a = "class A extends B {\n function Test() {\n  this.Secret();\n }\n}\n";
    let source_b = "class B {\n private function Secret() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 8,
        },
    );

    assert!(
        definition.is_none(),
        "private method of parent should not resolve from subclass"
    );
}

#[test]
fn private_method_visible_within_own_class() {
    let source =
        "class A {\n private function Secret() {}\n function Test() {\n  this.Secret();\n }\n}\n";
    let doc = parse_document(source).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc.symbols);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 3,
            character: 8,
        },
    )
    .expect("private method should be visible from within the same class");

    assert_eq!(definition.symbol.name, "Secret");
}

#[test]
fn protected_method_visible_in_subclass() {
    let source_a = "class A extends B {\n function Test() {\n  this.Guarded();\n }\n}\n";
    let source_b = "class B {\n protected function Guarded() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 8,
        },
    )
    .expect("protected method should be visible in a subclass");

    assert_eq!(definition.symbol.name, "Guarded");
}

#[test]
fn protected_method_not_visible_externally() {
    let source_a = "class A {\n function Test(b : B) {\n  b.Guarded();\n }\n}\n";
    let source_b = "class B {\n protected function Guarded() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 5,
        },
    );

    assert!(
        definition.is_none(),
        "protected method should not resolve from an unrelated external class"
    );
}

#[test]
fn unspecified_access_defaults_to_public() {
    let source_a = "class A {\n function Test(b : B) {\n  b.Open();\n }\n}\n";
    let source_b = "class B {\n function Open() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);

    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 5,
        },
    )
    .expect("method with no specifier should default to public and be visible externally");

    assert_eq!(definition.symbol.name, "Open");
}

#[test]
fn state_parent_dot_resolves_to_owner_class_method() {
    // parent.X inside a state should resolve to X on the owning class (public only).
    let source = concat!(
        "class CPlayer {\n",
        "  function GetHealth() : int {}\n",
        "}\n",
        "state Idle in CPlayer {\n",
        "  function Test() {\n",
        "    parent.GetHealth();\n",
        "  }\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc.symbols);

    // cursor on 'GetHealth' (line 5, col 11)
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 11,
        },
    )
    .expect("parent.Method() in a state should resolve to the owner class method");

    assert_eq!(definition.symbol.name, "GetHealth");
}

#[test]
fn state_parent_dot_cannot_see_protected_owner_method() {
    // parent confers no inheritance relationship; protected members of the owner
    // are not accessible via parent.
    let source = concat!(
        "class CPlayer {\n",
        "  protected function InternalTick() {}\n",
        "}\n",
        "state Idle in CPlayer {\n",
        "  function Test() {\n",
        "    parent.InternalTick();\n",
        "  }\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc.symbols);

    // cursor on 'InternalTick' (line 5, col 11)
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 11,
        },
    );

    assert!(
        definition.is_none(),
        "parent.X in a state must not resolve protected members of the owner class"
    );
}

#[test]
fn resolves_method_on_function_return_value() {
    let source = concat!(
        "class Duck {\n",
        "  public function Quack() {}\n",
        "}\n",
        "function ReturnsDuck() : Duck {}\n",
        "function Test() {\n",
        "  ReturnsDuck().Quack();\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc.symbols);

    // cursor on 'Quack' — line 5, col 16
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 16,
        },
    )
    .expect("Quack should resolve via return type of ReturnsDuck");

    assert_eq!(definition.symbol.name, "Quack");
    let container_id = definition.symbol.container.expect("method has a container");
    let container = doc.symbols.by_id(container_id).expect("container exists");
    assert_eq!(container.name, "Duck");
}

#[test]
fn find_references_for_private_member_scoped_to_defining_file() {
    let source_a = concat!(
        "class A {\n",
        "  private function Secret() {}\n",
        "  function Test() {\n",
        "    this.Secret();\n",
        "  }\n",
        "}\n",
    );
    let source_b = "function Secret() {}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a.symbols);
    index.update_document("file:///b.ws", &doc_b.symbols);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Resolve definition of 'Secret' at declaration site (line 1, col 20)
    // "  private function Secret() {}" — 'S' is at col 19
    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &db,
        SourcePosition {
            line: 1,
            character: 20,
        },
    )
    .expect("private method should resolve at definition site");

    assert_eq!(definition.symbol.name, "Secret");
    assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Method);

    let search_docs = vec![("file:///a.ws", &doc_a), ("file:///b.ws", &doc_b)];
    let refs = super::find_references(&definition, &doc_a, &search_docs, &db, false);

    // Only the call site in a.ws should appear; the top-level function in b.ws must not
    assert_eq!(refs.len(), 1, "reference in b.ws must not be included");
    assert!(
        refs[0].0 == "file:///a.ws",
        "sole reference must be in the defining file"
    );
}

#[test]
fn resolves_chained_call_method() {
    // ReturnsDuck().GetNest().Count() — two levels of chaining
    let source = concat!(
        "class Nest {\n",
        "  public function Count() : int {}\n",
        "}\n",
        "class Duck {\n",
        "  public function GetNest() : Nest {}\n",
        "}\n",
        "function ReturnsDuck() : Duck {}\n",
        "function Test() {\n",
        "  ReturnsDuck().GetNest().Count();\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc.symbols);

    // cursor on 'Count' — line 8, col 26
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 8,
            character: 26,
        },
    )
    .expect("Count should resolve via chained return types");

    assert_eq!(definition.symbol.name, "Count");
    let container_id = definition.symbol.container.expect("method has a container");
    let container = doc.symbols.by_id(container_id).expect("container exists");
    assert_eq!(container.name, "Nest");
}

fn make_env(name: &str, type_name: &str) -> ScriptEnvironment {
    use crate::line_index::SourceRange;
    use crate::script_env::ScriptGlobal;
    use crate::symbols::{Symbol, SymbolId, SymbolKind};
    let pos = SourcePosition {
        line: 1,
        character: 0,
    };
    let end = SourcePosition {
        line: 1,
        character: name.len() as u32,
    };
    ScriptEnvironment {
        globals: vec![ScriptGlobal {
            name: name.to_string(),
            type_name: type_name.to_string(),
            ini_uri: "file:///redscripts.ini".to_string(),
            symbol: Symbol {
                id: SymbolId(0),
                name: name.to_string(),
                kind: SymbolKind::Variable,
                range: SourceRange { start: pos, end },
                selection_range: SourceRange { start: pos, end },
                byte_range: 0..name.len(),
                selection_byte_range: 0..name.len(),
                container: None,
                container_name: None,
                type_annotation: Some(type_name.to_string()),
                signature: None,
                detail: None,
                annotations: Vec::new(),
                access: AccessLevel::Public,
            },
        }],
    }
}

#[test]
fn script_global_resolves_to_ini_when_class_not_loaded() {
    let doc = parse_document("function Test() {\n theGame;\n}\n").expect("parse");
    let env = make_env("theGame", "CR4Game");
    let workspace = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&workspace, &base).with_script_env(&env),
        SourcePosition {
            line: 1,
            character: 2,
        },
    )
    .expect("should resolve to ini");
    assert_eq!(def.uri, "file:///redscripts.ini");
    assert_eq!(def.symbol.name, "theGame");
}

#[test]
fn script_global_redirects_to_class_when_loaded() {
    let doc = parse_document("function Test() {\n theGame;\n}\n").expect("parse");
    let class_doc = parse_document("class CR4Game {}\n").expect("parse");
    let env = make_env("theGame", "CR4Game");
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///r4game.ws", &class_doc.symbols);
    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&WorkspaceIndex::default(), &base).with_script_env(&env),
        SourcePosition {
            line: 1,
            character: 2,
        },
    )
    .expect("should redirect to class");
    assert_eq!(def.symbol.name, "CR4Game");
    assert_eq!(def.uri, "file:///r4game.ws");
}

#[test]
fn member_access_on_script_global_resolves_method() {
    let doc = parse_document("function Test() {\n theGame.GetPlayer();\n}\n").expect("parse");
    let class_doc =
        parse_document("class CR4Game {\n public function GetPlayer() : CR4Player {}\n}\n")
            .expect("parse");
    let env = make_env("theGame", "CR4Game");
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///r4game.ws", &class_doc.symbols);
    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&WorkspaceIndex::default(), &base).with_script_env(&env),
        SourcePosition {
            line: 1,
            character: 11,
        },
    )
    .expect("GetPlayer should resolve");
    assert_eq!(def.symbol.name, "GetPlayer");
}

#[test]
fn local_var_with_same_name_as_script_global_resolves_to_local() {
    let doc = parse_document("function Test() {\n    var theGame : CR4Game;\n    theGame;\n}\n")
        .expect("parse");
    let class_doc = parse_document("class CR4Game {}\n").expect("parse");
    let env = make_env("theGame", "CR4Game");
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///r4game.ws", &class_doc.symbols);
    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&WorkspaceIndex::default(), &base).with_script_env(&env),
        SourcePosition {
            line: 2,
            character: 4,
        },
    )
    .expect("should resolve to local variable");
    assert_eq!(
        def.symbol.kind,
        crate::symbols::SymbolKind::Variable,
        "expected local variable, not class"
    );
    assert_eq!(def.symbol.name, "theGame");
    assert_ne!(
        def.uri, "file:///r4game.ws",
        "should not redirect to class when a local shadows the global"
    );
}
