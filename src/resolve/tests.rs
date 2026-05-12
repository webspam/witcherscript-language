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
    index.update_document("file:///test.ws", &document);

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
    index.update_document("file:///test.ws", &document);

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
    index.update_document("file:///test.ws", &document);

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
    let doc = parse_document(source).expect("parse should succeed");
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
    index.update_document("file:///a.ws", &doc);

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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

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
fn resolves_super_keyword_with_caret_at_end_of_word() {
    let source_a = "class A extends B {\n function Test() {\n  super.Method();\n }\n}\n";
    let source_b = "class B {\n function Method() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    // cursor just past the 'r' of "super" (line 2, col 7 — one past the end of the word)
    let definition = resolve_definition(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 7,
        },
    )
    .expect("super keyword should resolve when caret is at end of word");

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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

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
    index.update_document("file:///a.ws", &doc);
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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

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
    index.update_document("file:///test.ws", &doc);

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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

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
    index.update_document("file:///a.ws", &doc);

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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

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
    index.update_document("file:///test.ws", &doc);

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
    index.update_document("file:///test.ws", &doc);

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
    index.update_document("file:///test.ws", &doc);

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
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);
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
    index.update_document("file:///test.ws", &doc);

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

#[test]
fn completion_after_dot_returns_public_members() {
    let source = concat!(
        "class CPlayer {\n",
        "  public function GetHealth() : int {}\n",
        "  private var mHp : int;\n",
        "}\n",
        "function Test() {\n",
        "  var p : CPlayer;\n",
        "  p.\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // position is at the character after '.' on line 6
    let members = super::completion_members(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 6,
            character: 4,
        },
    );

    let names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    assert!(
        names.contains(&"GetHealth"),
        "public method should be in completions"
    );
    assert!(
        !names.contains(&"mHp"),
        "private field should not be in completions"
    );
}

#[test]
fn completion_includes_inherited_members() {
    let source_a = concat!(
        "class A extends B {\n",
        "  public function Own() {}\n",
        "}\n",
        "function Test() {\n",
        "  var a : A;\n",
        "  a.\n",
        "}\n",
    );
    let source_b = "class B {\n  public function Inherited() {}\n}\n";
    let doc_a = parse_document(source_a).expect("parse should succeed");
    let doc_b = parse_document(source_b).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);

    let members = super::completion_members(
        "file:///a.ws",
        &doc_a,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 4,
        },
    );

    let names: Vec<&str> = members
        .iter()
        .map(|(_, d)| d.symbol.name.as_str())
        .collect();
    assert!(names.contains(&"Own"), "own method should appear");
    assert!(
        names.contains(&"Inherited"),
        "inherited method should appear"
    );
    let own_tier = members
        .iter()
        .find(|(_, d)| d.symbol.name == "Own")
        .map(|(t, _)| *t)
        .unwrap();
    let inherited_tier = members
        .iter()
        .find(|(_, d)| d.symbol.name == "Inherited")
        .map(|(t, _)| *t)
        .unwrap();
    assert!(
        own_tier < inherited_tier,
        "own members must have lower sort tier than inherited members"
    );
}

#[test]
fn type_completions_offered_in_type_annotation() {
    // "var x : CP" with a complete statement on the next line gives tree-sitter
    // enough context to recover and emit a type_annot node for the partial name.
    let source = concat!(
        "class CPlayer {}\n",
        "struct SData {}\n",
        "enum EDir { North = 0 }\n",
        "function Test() {\n",
        "  var x : CP\n",
        "  var y : int;\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let types = super::type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 4,
            character: 11,
        },
    );

    let names: Vec<&str> = types.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CPlayer"),
        "class should be in type completions"
    );
    assert!(
        names.contains(&"SData"),
        "struct should be in type completions"
    );
    assert!(
        names.contains(&"EDir"),
        "enum should be in type completions"
    );
}

#[test]
fn type_completions_not_offered_inside_string_literal() {
    // The unterminated string causes an ERROR node — no type_annot ancestor exists,
    // so completions must not fire. CPlayer is indexed to prove the guard is what
    // suppresses it, not an empty type list.
    let source = concat!("class CPlayer {}\n", "function SomeFunc(\"test:\n",);
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let types = super::type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 24,
        },
    );

    assert!(
        types.is_empty(),
        "colon inside a string literal must not trigger type completion"
    );
}

#[test]
fn type_completions_not_offered_outside_type_context() {
    let source = "function Test() {\n  someVar\n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let index = WorkspaceIndex::default();

    let types = super::type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 9,
        },
    );

    assert!(
        types.is_empty(),
        "no type completions outside a type annotation"
    );
}

#[test]
fn type_completions_offered_cursor_right_of_complete_type_name() {
    // Regression: cursor positioned after a complete type name must still offer
    // completions. The byte offset lands on ';'; the type name is found via the -1 fallback.
    let source = "class CMyType {}\nfunction F() {\n  var z:CMyType;\n  var w : int;\n}\n";
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let types = super::type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 15,
        },
    );
    assert!(
        !types.is_empty(),
        "cursor right of a complete type name must still offer completions"
    );
}

#[test]
fn type_completions_offered_cursor_right_of_last_type_in_error_recovery() {
    // "var z : A : B : CMyType;" is a syntax error — tree-sitter only produces a
    // type_annot node for the final ": CMyType"; the earlier ": A" and ": B" become
    // ERROR nodes. Completions must still work at and after "CMyType".
    let source =
        "class CMyType {}\nfunction F() {\n  var z : A : B : CMyType;\n  var w : int;\n}\n";
    let doc = parse_document(source).expect("parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let types_at = super::type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 18,
        },
    );
    assert!(
        !types_at.is_empty(),
        "cursor at the start of the final type name must offer completions"
    );

    let types_after = super::type_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 2,
            character: 25,
        },
    );
    assert!(
        !types_after.is_empty(),
        "cursor right of the final type name must offer completions"
    );
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
                base_class: None,
                owner_class: None,
                flavour: None,
                annotations: Vec::new(),
                access: AccessLevel::Public,
                is_optional: false,
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
    base.update_document("file:///r4game.ws", &class_doc);
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
    base.update_document("file:///r4game.ws", &class_doc);
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
    base.update_document("file:///r4game.ws", &class_doc);
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

#[test]
fn parameters_of_returns_names_in_source_order() {
    let doc = parse_document(
        "function Find(findName : string, range : float, shouldScanAllObjects : bool) : int {}",
    )
    .expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db.find_top_level("Find").expect("Find should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["findName", "range", "shouldScanAllObjects"]);
}

#[test]
fn parameters_of_returns_empty_for_zero_param_function() {
    let doc = parse_document("function NoArgs() {}").expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db
        .find_top_level("NoArgs")
        .expect("NoArgs should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert!(params.is_empty());
}

#[test]
fn parameters_of_works_for_class_method() {
    let doc = parse_document("class CPlayer { function GetHealth(modifier : float) : int {} }")
        .expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db
        .find_member("CPlayer", "GetHealth", AccessLevel::Public)
        .expect("GetHealth should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["modifier"]);
}

#[test]
fn parameters_of_works_for_event() {
    let doc = parse_document("class C { event OnSpawn(spawnData : int) {} }")
        .expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db
        .find_member("C", "OnSpawn", AccessLevel::Public)
        .expect("OnSpawn should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["spawnData"]);
}

#[test]
fn parameters_of_skips_optional_params() {
    let doc = parse_document("function Find(name : string, optional range : float) : int {}")
        .expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db.find_top_level("Find").expect("Find should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["name"]);
}

#[test]
fn parameters_of_multi_name_group() {
    let doc =
        parse_document("function Multi(a, b : int, c : string) {}").expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let def = db.find_top_level("Multi").expect("Multi should be indexed");
    let params = db.parameters_of(&def.uri, def.symbol.id);

    assert_eq!(params, vec!["a", "b", "c"]);
}

#[test]
fn statement_completions_excludes_local_declared_after_cursor() {
    let source = "function Test() {\n  var bar : int;\n  bar;\n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor at line 1, character 2 — before the `bar` identifier in the declaration
    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    let local_names: Vec<&str> = result
        .locals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        !local_names.contains(&"bar"),
        "variable declared after cursor must not appear in locals"
    );
}

#[test]
fn statement_completions_includes_local_declared_before_cursor() {
    let source = "function Test() {\n  var count : int;\n  count;\n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor at line 2, character 2 — after the `count` declaration on line 1
    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 2,
        },
    );
    let local_names: Vec<&str> = result
        .locals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        local_names.contains(&"count"),
        "variable declared before cursor must appear in locals"
    );
}

#[test]
fn statement_completions_includes_parameters() {
    let source = "function Test(owner : int) {\n  owner;\n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    let local_names: Vec<&str> = result
        .locals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        local_names.contains(&"owner"),
        "function parameter must appear in locals"
    );
    assert!(
        result
            .locals
            .iter()
            .any(|d| d.symbol.name == "owner"
                && d.symbol.kind == crate::symbols::SymbolKind::Parameter),
        "owner must have kind Parameter"
    );
}

#[test]
fn statement_completions_members_includes_private_symbols_of_own_class() {
    let source = concat!(
        "class CExample {\n",
        "  private var secret : int;\n",
        "  private function Hidden() {}\n",
        "  function Test() {\n",
        "    secret;\n",
        "  }\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor on line 4, character 4 — inside the Test method body
    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 4,
        },
    );
    let member_names: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"secret"),
        "private field should appear in members when inside the class"
    );
    assert!(
        member_names.contains(&"Hidden"),
        "private method should appear in members when inside the class"
    );
}

#[test]
fn statement_completions_members_empty_in_free_function() {
    let source = "function Test() {\n  \n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    assert!(
        result.members.is_empty(),
        "members bucket must be empty when cursor is in a free function"
    );
}

#[test]
fn statement_completions_globals_contains_functions_from_indexed_documents() {
    let doc_a = parse_document("function Alpha() {}\n").expect("parse should succeed");
    let doc_b = parse_document("function Beta() {}\n").expect("parse should succeed");
    let doc_c = parse_document("function Caller() {\n  \n}\n").expect("parse should succeed");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc_a);
    index.update_document("file:///b.ws", &doc_b);
    index.update_document("file:///c.ws", &doc_c);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::statement_completions(
        "file:///c.ws",
        &doc_c,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    let global_names: Vec<&str> = result
        .globals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        global_names.contains(&"Alpha"),
        "Alpha from another document must appear in globals"
    );
    assert!(
        global_names.contains(&"Beta"),
        "Beta from another document must appear in globals"
    );
}

#[test]
fn statement_completions_globals_excludes_class_methods() {
    let source = concat!(
        "class Foo {\n",
        "  function Bar() {}\n",
        "}\n",
        "function Outer() {\n",
        "  \n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 2,
        },
    );
    let global_names: Vec<&str> = result
        .globals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        !global_names.contains(&"Bar"),
        "class method Bar must not appear in globals"
    );
}

#[test]
fn statement_completions_all_empty_outside_any_callable() {
    let source = "class CExample {}\n\n";
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor at line 1, character 0 — between definitions, not inside any callable
    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 0,
        },
    );
    assert!(
        result.locals.is_empty() && result.members.is_empty() && result.globals.is_empty(),
        "all buckets must be empty when cursor is outside any callable"
    );
}

#[test]
fn statement_completions_members_includes_inherited_public_method() {
    let source_b = "class B {\n  public function BMethod() {}\n}\n";
    let source_a = "class A extends B {\n  function Test() {\n    \n  }\n}\n";
    let doc_b = parse_document(source_b).expect("parse b");
    let doc_a = parse_document(source_a).expect("parse a");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///b.ws", &doc_b);
    index.update_document("file:///a.ws", &doc_a);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor at line 2, character 4 — inside A::Test method body
    let result = super::statement_completions(
        "file:///a.ws",
        &doc_a,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    );
    let member_names: Vec<&str> = result
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"BMethod"),
        "inherited public method from parent class must appear in members"
    );
}

#[test]
fn statement_completions_globals_excludes_exec_and_quest_functions() {
    let source = concat!(
        "exec function DebugCmd() {}\n",
        "quest function QuestFunc() {}\n",
        "function NormalFunc() {}\n",
        "function Caller() {\n",
        "  \n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 2,
        },
    );
    let global_names: Vec<&str> = result
        .globals
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        !global_names.contains(&"DebugCmd"),
        "exec function must not appear in globals"
    );
    assert!(
        !global_names.contains(&"QuestFunc"),
        "quest function must not appear in globals"
    );
    assert!(
        global_names.contains(&"NormalFunc"),
        "normal function must still appear in globals"
    );
}

#[test]
fn statement_completions_has_this_inside_class_method() {
    let source = "class CExample {\n  function Test() {\n    \n  }\n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    );
    assert!(
        result.has_this,
        "this must be available inside a class method"
    );
    assert!(
        !result.has_super,
        "super must not be available without a superclass"
    );
}

#[test]
fn statement_completions_has_super_when_class_extends() {
    let source_b = "class B {}\n";
    let source_a = "class A extends B {\n  function Test() {\n    \n  }\n}\n";
    let doc_b = parse_document(source_b).expect("parse b");
    let doc_a = parse_document(source_a).expect("parse a");

    let mut index = WorkspaceIndex::default();
    index.update_document("file:///b.ws", &doc_b);
    index.update_document("file:///a.ws", &doc_a);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::statement_completions(
        "file:///a.ws",
        &doc_a,
        &db,
        SourcePosition {
            line: 2,
            character: 4,
        },
    );
    assert!(
        result.has_this,
        "this must be available inside a class method"
    );
    assert!(
        result.has_super,
        "super must be available when class extends another"
    );
}

#[test]
fn statement_completions_no_this_in_free_function() {
    let source = "function Test() {\n  \n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 2,
        },
    );
    assert!(
        !result.has_this,
        "this must not be available in a free function"
    );
    assert!(
        !result.has_super,
        "super must not be available in a free function"
    );
}

#[test]
fn statement_completions_empty_after_dot_in_class_method() {
    // Regression: typing `someVar.` inside a class method must not trigger
    // statement completions — that belongs to completion_members.
    let source = concat!(
        "class CExample {\n",
        "  var mField : int;\n",
        "  function Test() {\n",
        "    var local : CExample;\n",
        "    local.\n",
        "  }\n",
        "}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // Cursor right after the dot on line 4 ("    local." = chars 0-9, dot at 9, cursor at 10).
    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 4,
            character: 10,
        },
    );
    assert!(
        result.locals.is_empty()
            && result.members.is_empty()
            && result.globals.is_empty()
            && !result.has_this
            && !result.has_super,
        "dot-access context must not produce statement completions"
    );
}

#[test]
fn statement_completions_empty_after_leading_dot_in_method() {
    // A bare '.' at the start of a statement has no valid LHS — tree-sitter
    // produces an incomplete_member_access_expr with a missing receiver.
    // statement_completions must not fire here.
    let source = "class C {\n  function A() {\n    .\n  }\n}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 2: "    ." — dot at col 4, cursor at col 5 (right after the dot)
    let result = super::statement_completions(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 2,
            character: 5,
        },
    );
    assert!(
        result.locals.is_empty()
            && result.members.is_empty()
            && result.globals.is_empty()
            && !result.has_this
            && !result.has_super,
        "a leading dot with no LHS must not produce statement completions"
    );
}

// --- Completion context detection tests ---
//
// Fixture layout for completion_class_body_contexts.ws:
//   line 0:  class C {
//   line 1:    var field : int;
//   line 2:    (blank)                    ← class body, outside any callable
//   line 3:    function Name(test : bool) {
//   line 4:    (blank)                    ← method body, inside callable
//   line 5:    }
//   line 6:  }
//
// Fixture layout for completion_declaration_contexts.ws:
//   line 0:  class CRefType {}
//   line 1:  (blank)
//   line 2:  class C {
//   line 3:    var isName : CRefType;     col 15 = start of CRefType (field type)
//   line 4:    private function SomeFunction() {}
//                                         col 19 = start of SomeFunction (fn name decl)
//   line 5:    var someVar : int;         col  6 = start of someVar (var name, between callables)
//                                         col 16 = start of int (field type)
//   line 6:    function Name(test : int, other : bool) {}
//                                         col 16 = start of test  (1st param name)
//                                         col 23 = start of int   (1st param type)
//                                         col 28 = start of other (2nd param name)
//                                         col 36 = start of bool  (2nd param type)
//   line 7:  }

#[test]
fn blank_in_class_body_yields_no_completions() {
    let source = include_str!("../../tests/fixtures/valid/completion_class_body_contexts.ws");
    let doc = parse_document(source).expect("fixture should parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 2,
        character: 0,
    };

    let members = super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire on a blank line in the class body"
    );

    let types = super::type_completions(&doc, &db, pos);
    assert!(
        types.is_empty(),
        "type completions must not fire on a blank line in the class body"
    );

    let stmt = super::statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty()
            && !stmt.has_this
            && !stmt.has_super,
        "statement completions must be all-empty in the class body (no enclosing callable)"
    );
}

#[test]
fn blank_in_class_method_body_yields_statement_completions() {
    let source = include_str!("../../tests/fixtures/valid/completion_class_body_contexts.ws");
    let doc = parse_document(source).expect("fixture should parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 4,
        character: 0,
    };

    let members = super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire without a dot"
    );

    let types = super::type_completions(&doc, &db, pos);
    assert!(
        types.is_empty(),
        "type completions must not fire without a type annotation"
    );

    let stmt = super::statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.has_this,
        "this must be available inside a class method body"
    );
    let local_names: Vec<&str> = stmt.locals.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        local_names.contains(&"test"),
        "function parameter 'test' must appear in locals at the blank line"
    );
    let member_names: Vec<&str> = stmt
        .members
        .iter()
        .map(|d| d.symbol.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"field"),
        "class field 'field' must appear in members inside the method"
    );
}

#[test]
fn field_type_annotation_in_class_body_fires_type_completions() {
    let source = include_str!("../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = parse_document(source).expect("fixture should parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 15: start of CRefType in `  var isName : CRefType;`
    let pos = SourcePosition {
        line: 3,
        character: 15,
    };

    let members = super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a field type annotation"
    );

    let types = super::type_completions(&doc, &db, pos);
    let type_names: Vec<&str> = types.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        type_names.contains(&"CRefType"),
        "CRefType must appear in type completions at the field type position"
    );

    let stmt = super::statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty()
            && !stmt.has_this,
        "statement completions must be all-empty at a class field type annotation (no enclosing callable)"
    );
}

#[test]
fn field_name_between_methods_yields_no_completions() {
    let source = include_str!("../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = parse_document(source).expect("fixture should parse");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 6: start of someVar in `  var someVar : int;` — between SomeFunction and Name
    let pos = SourcePosition {
        line: 5,
        character: 6,
    };

    let members = super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a variable name declaration"
    );

    let types = super::type_completions(&doc, &db, pos);
    assert!(
        types.is_empty(),
        "type completions must not fire at a variable name (not a type annotation)"
    );

    let stmt = super::statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty()
            && !stmt.has_this,
        "statement completions must be all-empty at a variable name between two methods"
    );
}

#[test]
fn field_type_between_methods_fires_type_completions() {
    let source = include_str!("../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = parse_document(source).expect("fixture should parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 16: start of int in `  var someVar : int;` — between SomeFunction and Name
    let pos = SourcePosition {
        line: 5,
        character: 16,
    };

    let members = super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a field type annotation"
    );

    let types = super::type_completions(&doc, &db, pos);
    assert!(
        !types.is_empty(),
        "type completions must fire at the field type position"
    );

    let stmt = super::statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty()
            && !stmt.has_this,
        "statement completions must be all-empty at a field type annotation between methods"
    );
}

#[test]
fn extends_completions_fires_after_extends_keyword_incomplete_decl() {
    // Source: class with no body — whole decl is an ERROR node in the tree.
    // Cursor is right after the space following 'extends'.
    let source = "class CExample {}\nclass Foo extends \n";
    //            line 0              line 1: "class Foo extends " (chars 0-17, cursor at 18)
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 1,
        character: 18,
    };

    let result = super::extends_completions(&doc, &db, pos);
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CExample"),
        "class names must appear in extends completions after the 'extends' keyword"
    );
}

#[test]
fn extends_completions_fires_mid_base_class_name() {
    // Cursor is inside a partially-typed base class name — still an ERROR node.
    let source = "class CExample {}\nclass Foo extends CEx\n";
    //            line 1: "class Foo extends CEx" — cursor at char 20 (on 'x')
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 1,
        character: 20,
    };

    let result = super::extends_completions(&doc, &db, pos);
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CExample"),
        "extends completions must fire while mid-typing the base class name"
    );
}

#[test]
fn extends_completions_empty_inside_class_body() {
    // Cursor is inside the class body — must NOT trigger extends completions.
    let source = "class CExample {}\nclass Foo extends CExample {\n  \n}\n";
    //            line 2: "  " — cursor at char 2 (inside body)
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 2,
        character: 2,
    };

    let result = super::extends_completions(&doc, &db, pos);
    assert!(
        result.is_empty(),
        "extends completions must not fire inside the class body"
    );
}

#[test]
fn extends_completions_empty_at_class_name_position() {
    // Cursor is on the class name itself — no 'extends' keyword present.
    let source = "class Foo {\n  \n}\n";
    //            line 0: "class Foo {" — cursor at char 6 (on 'F')
    let doc = parse_document(source).expect("parse should succeed");
    let index = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 0,
        character: 6,
    };

    let result = super::extends_completions(&doc, &db, pos);
    assert!(
        result.is_empty(),
        "extends completions must not fire at the class name position"
    );
}

#[test]
fn extends_completions_fires_for_state_decl() {
    // State declarations also support 'extends' for base state.
    let source = "class CBase {}\nstate IdleState in CBase extends \n";
    //            line 1: "state IdleState in CBase extends " (32 chars, cursor at 32)
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let pos = SourcePosition {
        line: 1,
        character: 32,
    };

    let result = super::extends_completions(&doc, &db, pos);
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CBase"),
        "extends completions must fire after 'extends' in a state declaration"
    );
}

#[test]
fn extends_completions_excludes_enums_and_structs() {
    // Only Class and State symbols must appear — not Enum or Struct.
    let source = concat!(
        "class CExample {}\n",
        "struct SExample {}\n",
        "enum EExample {}\n",
        "class Foo extends \n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 3: "class Foo extends " — cursor at char 18
    let pos = SourcePosition {
        line: 3,
        character: 18,
    };

    let result = super::extends_completions(&doc, &db, pos);
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CExample"),
        "Class symbols must appear in extends completions"
    );
    assert!(
        !names.contains(&"SExample"),
        "Struct symbols must not appear in extends completions"
    );
    assert!(
        !names.contains(&"EExample"),
        "Enum symbols must not appear in extends completions"
    );
}

#[test]
fn extends_completions_empty_between_extends_and_class_body() {
    // "class C extends  {}" — two spaces between 'extends' and '{'
    // Cursor is in the second space (line 1, char 16). No base class name has been typed,
    // but the body {} is already present. Must not fire completions.
    let source = "class CExample {}\nclass C extends  {}\n";
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    let result = super::extends_completions(
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 16,
        },
    );
    let names: Vec<&str> = result.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        names.contains(&"CExample"),
        "extends completions must fire when cursor is in whitespace between 'extends' and the class body, so the user can pick a base class"
    );
}

#[test]
fn parameter_type_annotation_fires_type_completions() {
    // `CParam` is in the parameter type annotation — must trigger type completions
    // regardless of whether the enclosing callable is a free function or a method.
    let source = concat!("class CParam {}\n", "function Foo(x : CParam) {}\n",);
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 1: `function Foo(x : CParam) {}`
    //          0         1
    //          0123456789012345678
    //                            ^ col 17 = start of CParam
    let pos = SourcePosition {
        line: 1,
        character: 17,
    };

    let members = super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a parameter type annotation"
    );

    let types = super::type_completions(&doc, &db, pos);
    let type_names: Vec<&str> = types.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        type_names.contains(&"CParam"),
        "CParam must appear in type completions at the parameter type position"
    );
}

#[test]
fn function_return_type_annotation_fires_type_completions() {
    let source = concat!(
        "class CReturnType {}\n",
        "function Foo() : CReturnType {}\n",
    );
    let doc = parse_document(source).expect("parse should succeed");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // line 1: `function Foo() : CReturnType {}`
    //          0         1
    //          01234567890123456789
    //                             ^ col 17 = start of CReturnType
    let pos = SourcePosition {
        line: 1,
        character: 17,
    };

    let members = super::completion_members("file:///test.ws", &doc, &db, pos);
    assert!(
        members.is_empty(),
        "dot-access completions must not fire at a return type annotation"
    );

    let types = super::type_completions(&doc, &db, pos);
    let type_names: Vec<&str> = types.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(
        type_names.contains(&"CReturnType"),
        "CReturnType must appear in type completions at the return type position"
    );
}

#[test]
fn function_name_in_class_body_yields_no_statement_completions() {
    let source = include_str!("../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = parse_document(source).expect("fixture should parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 19: start of SomeFunction in `  private function SomeFunction() {}`
    let pos = SourcePosition {
        line: 4,
        character: 19,
    };

    let stmt = super::statement_completions("file:///test.ws", &doc, &db, pos);
    assert!(
        stmt.locals.is_empty()
            && stmt.members.is_empty()
            && stmt.globals.is_empty()
            && !stmt.has_this,
        "statement completions must be all-empty at a function name declaration"
    );
}

#[test]
fn parameter_name_yields_no_statement_completions() {
    let source = include_str!("../../tests/fixtures/valid/completion_declaration_contexts.ws");
    let doc = parse_document(source).expect("fixture should parse");
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);

    // col 16: start of `test`  (first parameter name)
    // col 28: start of `other` (second parameter name, after the comma)
    for (label, character) in [("first param", 16u32), ("second param after comma", 28u32)] {
        let pos = SourcePosition { line: 6, character };
        let stmt = super::statement_completions("file:///test.ws", &doc, &db, pos);
        assert!(
            stmt.locals.is_empty()
                && stmt.members.is_empty()
                && stmt.globals.is_empty()
                && !stmt.has_this,
            "statement completions must be all-empty at parameter name ({label})"
        );
    }
}

// ── class_body_keyword_completions ──────────────────────────────────────────

fn kw(doc: &crate::document::ParsedDocument, line: u32, character: u32) -> Vec<&'static str> {
    super::class_body_keyword_completions(doc, SourcePosition { line, character })
}

#[test]
fn class_body_kw_blank_position_offers_all_keywords() {
    // Cursor on an otherwise empty line inside a class body.
    let source = "class CExample {\n  \n}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 1, 2);
    for expected in &[
        "var", "function", "event", "autobind", "private", "public", "import", "editable", "saved",
        "default", "defaults", "hint",
    ] {
        assert!(
            result.contains(expected),
            "blank class body should offer '{expected}'"
        );
    }
}

#[test]
fn class_body_kw_not_offered_inside_func_block() {
    let source = "class CExample {\n  function Foo() {\n    \n  }\n}\n";
    let doc = parse_document(source).expect("parse");
    // Line 2, col 4 is inside the function body — no class-body keywords.
    let result = kw(&doc, 2, 4);
    assert!(
        result.is_empty(),
        "must not offer class-body keywords inside a func_block"
    );
}

#[test]
fn class_body_kw_not_offered_outside_class() {
    let source = "function Foo() {}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 0, 0);
    assert!(
        result.is_empty(),
        "must not offer class-body keywords at top level"
    );
}

#[test]
fn class_body_kw_after_access_modifier_offers_decl_and_remaining_specifiers() {
    // "  private " — access seen, cursor after the space.
    let source = "class CExample {\n  private \n}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 1, 10);
    assert!(
        result.contains(&"var"),
        "should offer var after access modifier"
    );
    assert!(
        result.contains(&"function"),
        "should offer function after access modifier"
    );
    assert!(
        result.contains(&"autobind"),
        "should offer autobind after access modifier"
    );
    assert!(
        result.contains(&"final"),
        "should offer final after access modifier"
    );
    assert!(
        result.contains(&"latent"),
        "should offer latent after access modifier"
    );
    assert!(
        result.contains(&"editable"),
        "should offer editable after access modifier"
    );
    assert!(!result.contains(&"private"), "should not re-offer private");
    assert!(
        !result.contains(&"import"),
        "should not offer import after access modifier"
    );
}

#[test]
fn class_body_kw_after_editable_suppresses_func_keywords() {
    let source = "class CExample {\n  editable \n}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 1, 10);
    assert!(result.contains(&"var"), "should offer var after editable");
    assert!(
        !result.contains(&"function"),
        "function invalid after editable"
    );
    assert!(!result.contains(&"final"), "final invalid after editable");
    assert!(!result.contains(&"latent"), "latent invalid after editable");
    assert!(
        !result.contains(&"autobind"),
        "autobind invalid after editable"
    );
}

#[test]
fn class_body_kw_after_final_suppresses_var_and_autobind() {
    let source = "class CExample {\n  final \n}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 1, 8);
    assert!(
        result.contains(&"function"),
        "should offer function after final"
    );
    assert!(
        result.contains(&"latent"),
        "should offer latent after final"
    );
    assert!(!result.contains(&"var"), "var invalid after final");
    assert!(
        !result.contains(&"autobind"),
        "autobind invalid after final"
    );
    assert!(
        !result.contains(&"editable"),
        "editable invalid after final"
    );
}

#[test]
fn class_body_kw_after_import_suppresses_var_group_and_autobind() {
    let source = "class CExample {\n  import \n}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 1, 9);
    assert!(result.contains(&"var"), "should offer var after import");
    assert!(
        result.contains(&"function"),
        "should offer function after import"
    );
    assert!(
        result.contains(&"private"),
        "should offer access after import"
    );
    assert!(result.contains(&"final"), "should offer final after import");
    assert!(!result.contains(&"import"), "should not re-offer import");
    assert!(
        !result.contains(&"autobind"),
        "autobind invalid after import"
    );
    assert!(
        !result.contains(&"editable"),
        "editable invalid after import"
    );
}

#[test]
fn class_body_kw_after_decl_keyword_returns_empty() {
    // Once "var" has been typed the specifier phase is over.
    let source = "class CExample {\n  private var \n}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 1, 14);
    assert!(
        result.is_empty(),
        "no keyword completions after a declaration keyword"
    );
}

#[test]
fn class_body_kw_struct_does_not_offer_function_or_autobind() {
    let source = "struct SData {\n  \n}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 1, 2);
    assert!(result.contains(&"var"), "struct should offer var");
    assert!(
        !result.contains(&"function"),
        "struct must not offer function"
    );
    assert!(
        !result.contains(&"autobind"),
        "struct must not offer autobind"
    );
    assert!(!result.contains(&"event"), "struct must not offer event");
    assert!(!result.contains(&"final"), "struct must not offer final");
}

#[test]
fn class_body_kw_state_offers_same_as_class() {
    let source = "state SIdle in CPlayer {\n  \n}\n";
    let doc = parse_document(source).expect("parse");
    let result = kw(&doc, 1, 2);
    for expected in &["var", "function", "event", "autobind", "private", "final"] {
        assert!(
            result.contains(expected),
            "state body should offer '{expected}'"
        );
    }
}
