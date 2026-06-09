use rstest::rstest;

use super::super::{hover_text, resolve_all_definitions, resolve_definition};
use crate::symbols::SymbolKind;
use crate::test_support::TestDb;

#[rstest]
#[case::top_level_function_self_site(
    "function $0Foo() {}\n",
    "Foo",
    Some(SymbolKind::Function),
    None
)]
#[case::word_boundary_one_past_last_char("function Foo$0() {}\n", "Foo", None, None)]
#[case::class_method_self_site(
    "class CExample {\n function $0Bar() {}\n}\n",
    "Bar",
    Some(SymbolKind::Method),
    None
)]
#[case::enum_member_self_site(
    "enum EFoo {\n $0VALUE_A = 0\n}\n",
    "VALUE_A",
    Some(SymbolKind::EnumMember),
    None
)]
#[case::enum_member_cross_document(
    "//- /enums.ws\n\
     enum EColor { ERed = 0 }\n\
     //- /user.ws\n\
     function F() { var c : EColor; c = E$0Red; }\n",
    "ERed",
    Some(SymbolKind::EnumMember),
    Some("file:///enums.ws")
)]
#[case::receiver_variable_resolves_to_declaration(
    "class Example {\n  function Test() {\n    var unrelated : UnrelatedClass;\n    $0unrelated.Initialize();\n  }\n}\n",
    "unrelated",
    Some(SymbolKind::Variable),
    None
)]
#[case::parameter_shadows_top_level(
    "function value() {}\nfunction test(value : int) {\n $0value = 1;\n}\n",
    "value",
    Some(SymbolKind::Parameter),
    None
)]
#[case::add_method_resolves_from_call_site(
    "//- /base.ws\n\
     class CPlayer {\n  public function Heal() {}\n}\n\
     //- /a.ws\n\
     @addMethod(CPlayer)\nfunction Boost() {}\n\
     //- /caller.ws\n\
     function Caller() {\n  var p : CPlayer;\n  p.$0Boost();\n}\n",
    "Boost",
    None,
    Some("file:///a.ws")
)]
#[case::add_field_resolves_from_member_access(
    "//- /base.ws\n\
     class CPlayer {}\n\
     //- /a.ws\n\
     @addField(CPlayer) public var boost : int;\n\
     //- /caller.ws\n\
     function Caller() {\n  var p : CPlayer;\n  var x : int;\n  x = p.$0boost;\n}\n",
    "boost",
    Some(SymbolKind::Field),
    Some("file:///a.ws")
)]
#[case::add_field_chained_type(
    "//- /helper.ws\n\
     class CHelper {\n  public function Run() {}\n}\n\
     //- /base.ws\n\
     class CPlayer {}\n\
     //- /a.ws\n\
     @addField(CPlayer) public var helper : CHelper;\n\
     //- /caller.ws\n\
     function Caller() {\n  var p : CPlayer;\n  p.helper.$0Run();\n}\n",
    "Run",
    None,
    Some("file:///helper.ws")
)]
#[case::member_access_through_top_level_receiver(
    "//- /helper.ws\n\
     class CHelper {\n  public function Run() {}\n}\n\
     //- /main.ws\n\
     var gHelper : CHelper;\nfunction Test() {\n  gHelper.$0Run();\n}\n",
    "Run",
    Some(SymbolKind::Method),
    Some("file:///helper.ws")
)]
#[case::autobind_member_resolves_as_field(
    "class C {\n  autobind theInput : CInputManager = single;\n  function Use() {\n    $0theInput.Foo();\n  }\n}\n",
    "theInput",
    Some(SymbolKind::Field),
    Some("file:///main.ws")
)]
fn resolves_definition_at_cursor(
    #[case] fixture: &str,
    #[case] expected_name: &str,
    #[case] expected_kind: Option<SymbolKind>,
    #[case] expected_uri: Option<&str>,
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let def =
        resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("definition should resolve");
    assert_eq!(def.symbol.name, expected_name);
    if let Some(kind) = expected_kind {
        assert_eq!(def.symbol.kind, kind);
    }
    if let Some(u) = expected_uri {
        assert_eq!(def.uri, u);
    }
}

#[rstest]
#[case::unknown_receiver_does_not_fall_back(
    "class Example {\n  public function Initialize() {\n    typo.$0Initialize();\n  }\n}\n"
)]
fn resolve_yields_none(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    assert!(resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).is_none());
}

#[rstest]
#[case::initializer_refs_prior_local(
    "function Test() {\n  var source : int;\n  var x : int = $0source;\n}\n",
    Some(("source", SymbolKind::Variable, 1)),
)]
#[case::initializer_refs_parameter(
    "function Test(p : int) {\n  var x : int = $0p;\n}\n",
    Some(("p", SymbolKind::Parameter, 0)),
)]
#[case::initializer_undeclared_does_not_self_resolve(
    "function Test() {\n  var x : int = $0ghost;\n}\n",
    None
)]
fn goto_def_on_var_initializer(
    #[case] fixture: &str,
    #[case] expected: Option<(&str, SymbolKind, u32)>,
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let result = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos);
    match (result, expected) {
        (Some(def), Some((name, kind, decl_line))) => {
            assert_eq!(def.symbol.name, name);
            assert_eq!(def.symbol.kind, kind);
            assert_eq!(
                def.symbol.selection_range.start.line, decl_line,
                "must point at declaration, not the initializer use"
            );
        }
        (None, None) => {}
        (Some(def), None) => panic!(
            "expected None, got `{}` at line {}",
            def.symbol.name, def.symbol.selection_range.start.line
        ),
        (None, Some(_)) => panic!("expected resolution, got None"),
    }
}

#[rstest]
#[case::wrap_function_name_yields_all(
    "//- /base.ws\n\
     class CPlayer {\n  public function OnSpawned() {}\n}\n\
     //- /a.ws\n\
     @wrapMethod(CPlayer)\nfunction $0OnSpawned() {}\n",
    2, &["file:///base.ws", "file:///a.ws"],
)]
#[case::plain_method_has_single_declaration(
    "class CExample {\n  function $0Bar() {}\n}\n",
    1, &[],
)]
#[case::multiple_wraps_yield_all(
    "//- /base.ws\n\
     class CPlayer {\n  public function OnSpawned() {}\n}\n\
     //- /a.ws\n\
     @wrapMethod(CPlayer)\nfunction $0OnSpawned() {}\n\
     //- /b.ws\n\
     @wrapMethod(CPlayer)\nfunction OnSpawned() {}\n",
    3, &[],
)]
#[case::replace_method_included(
    "//- /base.ws\n\
     class CPlayer {\n  public function OnSpawned() {}\n}\n\
     //- /a.ws\n\
     @replaceMethod(CPlayer)\nfunction $0OnSpawned() {}\n",
    2, &["file:///a.ws"],
)]
#[case::wrap_unknown_class_returns_just_self(
    "@wrapMethod(CGhost)\nfunction $0Haunt() {}\n",
    1, &["file:///main.ws"],
)]
#[case::add_method_has_no_class_body_counterpart(
    "//- /base.ws\n\
     class CPlayer {\n  public function Heal() {}\n}\n\
     //- /a.ws\n\
     @addMethod(CPlayer)\nfunction Boost() {}\n\
     //- /caller.ws\n\
     function Caller() {\n  var p : CPlayer;\n  p.$0Boost();\n}\n",
    1, &["file:///a.ws"],
)]
fn resolve_all_definitions_at_cursor(
    #[case] fixture: &str,
    #[case] expected_count: usize,
    #[case] required_uris: &[&str],
) {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(
        defs.len(),
        expected_count,
        "actual uris: {:?}",
        defs.iter().map(|d| &d.uri).collect::<Vec<_>>()
    );
    for required in required_uris {
        assert!(
            defs.iter().any(|d| d.uri == *required),
            "missing required uri {required:?}"
        );
    }
}

#[test]
fn variable_dot_method_resolves_into_declared_type_not_current_class() {
    let t = TestDb::new(concat!(
        "class Example {\n",
        "  public function Initialize() {\n",
        "    var unrelated : UnrelatedClass = new UnrelatedClass in this;\n",
        "    unrelated.$0Initialize();\n",
        "  }\n",
        "}\n",
        "class UnrelatedClass {\n",
        "  public function Initialize() {}\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let def = resolve_definition(&uri, doc, &t.db(), pos)
        .expect("should resolve to UnrelatedClass.Initialize");
    assert_eq!(def.symbol.name, "Initialize");
    let container_id = def.symbol.container.expect("method must have container");
    let container = doc.symbols.by_id(container_id).expect("container exists");
    assert_eq!(container.name, "UnrelatedClass");
}

#[test]
fn resolves_enum_member_reference_in_expression() {
    let t = TestDb::new(
        "enum EColor { ERed = 0, EBlue = 1 }\nfunction F() { var c : EColor = E$0Red; }\n",
    );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("enum member reference in expression should resolve");
    assert_eq!(def.symbol.name, "ERed");
    assert_eq!(def.symbol.kind, SymbolKind::EnumMember);
    assert_eq!(def.symbol.selection_range.start.line, 0);
}

#[test]
fn goto_def_past_trailing_semicolon_resolves_left_identifier() {
    let t = TestDb::new(
        "class CInventoryComponent {}\n\
         class C {\n  import final function GetInventory() : CInventoryComponent;$0\n}\n",
    );
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(
        defs.len(),
        1,
        "should resolve the type name left of the trailing semicolon"
    );
    assert_eq!(defs[0].symbol.name, "CInventoryComponent");
    assert_eq!(defs[0].symbol.kind, SymbolKind::Class);
}

#[test]
fn goto_def_past_semicolon_ignores_non_identifier_before_it() {
    let t = TestDb::new(
        "function Make() {}\n\
         function F() {\n  Make();$0\n}\n",
    );
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert!(
        defs.is_empty(),
        "char left of the semicolon is ')', not an identifier"
    );
}

#[test]
fn goto_def_from_call_site_returns_class_body_and_wrap() {
    let t = TestDb::new(
        "//- /base.ws\n\
         class CPlayer {\n  public function OnSpawned() {}\n}\n\
         //- /a.ws\n\
         @wrapMethod(CPlayer)\nfunction OnSpawned() {}\n\
         //- /caller.ws\n\
         function Caller() {\n  var p : CPlayer;\n  p.O$0nSpawned();\n}\n",
    );
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
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
fn wrapped_method_macro_resolves_to_wrapped_method() {
    let t = TestDb::new(
        "//- /base.ws\n\
         class CPlayer {\n  public function OnSpawned() {}\n}\n\
         //- /a.ws\n\
         @wrapMethod(CPlayer)\nfunction OnSpawned() {\n  wrapped$0Method();\n}\n",
    );
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let def = resolve_definition(&uri, doc, &t.db(), pos)
        .expect("wrappedMethod should resolve to the wrapped method");
    assert_eq!(def.symbol.name, "OnSpawned");
    assert_eq!(def.symbol.kind, SymbolKind::Method);
    assert_eq!(def.symbol.container_name.as_deref(), Some("CPlayer"));
    assert!(
        hover_text(&def, &t.db()).contains("(method) CPlayer.OnSpawned"),
        "hover should describe the wrapped method"
    );

    let defs = resolve_all_definitions(&uri, doc, &t.db(), pos);
    assert_eq!(
        defs.iter().map(|d| d.uri.as_str()).collect::<Vec<_>>(),
        vec!["file:///base.ws"],
        "wrappedMethod resolves only to the wrapped method, not the wrapper"
    );
}

// Cursor past the `;` resolves through the wrappedMethod ident; suppression must follow that ident, not the offset.
#[test]
fn wrapped_method_macro_past_trailing_semicolon_suppresses_wrapper() {
    let t = TestDb::new(
        "//- /base.ws\n\
         class CPlayer {\n  public function OnSpawned() {}\n}\n\
         //- /a.ws\n\
         @wrapMethod(CPlayer)\nfunction OnSpawned() {\n  wrappedMethod; $0\n}\n",
    );
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(
        defs.iter().map(|d| d.uri.as_str()).collect::<Vec<_>>(),
        vec!["file:///base.ws"],
        "wrappedMethod must resolve only to the wrapped method, never the wrapper"
    );
}

#[test]
fn this_wrapped_method_member_access_does_not_redirect() {
    let t = TestDb::new(
        "class CPlayer {\n  public function OnSpawned() {}\n}\n\
         @wrapMethod(CPlayer)\nfunction OnSpawned() {\n  this.wrapped$0Method();\n}\n",
    );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos);
    assert!(
        def.is_none(),
        "this.wrappedMethod() is an ordinary member call, not the macro"
    );
}

#[test]
fn wrapped_method_outside_wrap_does_not_redirect() {
    let t = TestDb::new(
        "//- /base.ws\n\
         class CPlayer {\n  public function Heal() {}\n}\n\
         //- /a.ws\n\
         @addMethod(CPlayer)\nfunction Boost() {\n  wrapped$0Method();\n}\n",
    );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos);
    assert!(
        def.is_none(),
        "wrappedMethod outside a @wrapMethod body has nothing to wrap"
    );
}

// A real method declaration must win over a @wrapMethod overlay of the same name,
// even when the declaration lives in a lower-priority index (the base game scripts).
#[test]
fn wrapped_method_macro_resolves_to_base_class_body() {
    let t =
        TestDb::new("@wrapMethod(CR4Player)\nfunction OnSpawnHorse() {\n  wrapped$0Method();\n}\n")
            .with_base_doc(
                "file:///base/r4Player.ws",
                "class CR4Player {\n  public function OnSpawnHorse() {}\n}\n",
            );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("wrappedMethod should resolve to the wrapped method");
    assert_eq!(
        def.uri, "file:///base/r4Player.ws",
        "wrappedMethod must target the base class-body method, not the wrapper"
    );
    assert_eq!(def.symbol.kind, SymbolKind::Method);
    assert_eq!(def.symbol.container_name.as_deref(), Some("CR4Player"));
}

// `wrappedMethod` points only at the wrapped method, so the handler path must not fan out to the wrapper.
#[test]
fn wrapped_method_macro_yields_only_base_via_handler_path() {
    let t =
        TestDb::new("@wrapMethod(CR4Player)\nfunction OnSpawnHorse() {\n  wrapped$0Method();\n}\n")
            .with_base_doc(
                "file:///base/r4Player.ws",
                "class CR4Player {\n  public function OnSpawnHorse() {}\n}\n",
            );
    let (uri, pos) = t.cursor();
    let defs = resolve_all_definitions(&uri, t.doc_for(&uri), &t.db(), pos);
    assert_eq!(
        defs.iter().map(|d| d.uri.as_str()).collect::<Vec<_>>(),
        vec!["file:///base/r4Player.ws"],
        "wrappedMethod must resolve to the base method only, never the wrapper"
    );
}

// @addMethod has no class-body declaration anywhere; the annotation overlay is the
// only declaration and must still resolve.
#[test]
fn goto_def_on_added_method_call_resolves_to_annotation() {
    let t = TestDb::new(
        "//- /add.ws\n\
         @addMethod(CR4Player)\nfunction Boost() {}\n\
         //- /caller.ws\n\
         function test(p : CR4Player) {\n  p.Boo$0st();\n}\n",
    )
    .with_base_doc(
        "file:///base/r4Player.ws",
        "class CR4Player {\n  public function OnSpawnHorse() {}\n}\n",
    );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("added method should resolve to its annotation declaration");
    assert_eq!(def.uri, "file:///add.ws");
    assert_eq!(def.symbol.name, "Boost");
}
