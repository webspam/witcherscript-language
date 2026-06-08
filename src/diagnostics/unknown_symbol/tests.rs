use rstest::rstest;

use super::collect_unknown_symbol_diagnostics;
use crate::diagnostics::collect_cst_diagnostics_for_document;
use crate::document::parse_document;
use crate::resolve::{SymbolDb, WorkspaceIndex};
use crate::test_support::{TestDb, script_env};

#[test]
fn parallel_run_is_deterministic() {
    let mut src = String::new();
    for i in 0..40 {
        src.push_str(&format!(
            "class C{i} extends Missing{i} {{ var f{i} : MissingType{i}; }} \
             function Fn{i}() {{ var x{i} : int; x{i} = unknownBare{i}; UnknownCall{i}(); }} \
             function Fn2_{i}() {{ var c{i} : C{i}; c{i}.bogus{i} = 1; }}\n"
        ));
    }
    let doc = parse_document(&src).expect("parse should succeed");
    let mut idx = WorkspaceIndex::default();
    idx.update_document("file:///big.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&idx, &base);

    let first = collect_cst_diagnostics_for_document("file:///big.ws", &doc, &db);
    let second = collect_cst_diagnostics_for_document("file:///big.ws", &doc, &db);

    assert!(!first.is_empty(), "fixture should produce diagnostics");
    assert_eq!(
        first.len(),
        second.len(),
        "diagnostic count must be stable across runs"
    );
    for (i, (a, b)) in first.iter().zip(second.iter()).enumerate() {
        assert_eq!(a.kind, b.kind, "diagnostic {i}: kind mismatch");
        assert_eq!(a.message, b.message, "diagnostic {i}: message mismatch");
        assert_eq!(a.severity, b.severity, "diagnostic {i}: severity mismatch");
        assert_eq!(a.range, b.range, "diagnostic {i}: range mismatch");
    }
}

fn kinds(diags: &[super::WorkspaceDiagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.kind.as_str()).collect()
}

#[rstest]
#[case::declarations_do_not_fire(
    "class Foo {} \
     struct S {} \
     enum E { V } \
     function F(a, b : int) { var x, y : int; } \
     event Ev() {} \
     state St in Foo { entry function Run() {} }\n"
)]
#[case::builtin_types(
    "function F(a : bool, b : int, c : float, d : string, e : name, f : byte) : void {}\n"
)]
#[case::builtin_alias_bool("function F(a : Bool) {}\n")]
#[case::builtin_alias_float("function F(a : Float) {}\n")]
#[case::builtin_alias_string("function F(a : String) {}\n")]
#[case::builtin_alias_cname("function F(a : CName) {}\n")]
#[case::builtin_alias_int32("function F(a : Int32) {}\n")]
#[case::builtin_alias_uint8("function F(a : Uint8) {}\n")]
#[case::builtin_alias_int16("function F(a : Int16) {}\n")]
#[case::builtin_alias_int8("function F(a : Int8) {}\n")]
#[case::builtin_alias_uint32("function F(a : Uint32) {}\n")]
#[case::builtin_alias_uint16("function F(a : Uint16) {}\n")]
#[case::builtin_alias_stringansi("function F(a : StringAnsi) {}\n")]
#[case::known_type("class A {} class B extends A { var a : A; }\n")]
#[case::default_auto_state("statemachine class Player { default autoState = 'Exploration'; }\n")]
#[case::default_on_private_inherited(
    "class Super { private var hidden : int; default hidden = 1; } \
     class Sub extends Super { default hidden = 2; }\n"
)]
#[case::hint_on_private_inherited(
    "class Super { private var hidden : int; } \
     class Sub extends Super { hint hidden = \"tip\"; }\n"
)]
#[case::known_member("class A { var known : int; } function F() { var a : A; a.known = 1; }\n")]
#[case::private_member_inside_class(
    "class A { private var hidden : int; function R() { var a : A; a.hidden = 1; } }\n"
)]
#[case::private_member_inside_add_method(
    "class A { private var hidden : int; } \
     @addMethod(A) function R() { var a : A; a.hidden = 1; }\n"
)]
#[case::add_field_private_inside_add_method_same_class(
    "class Foo {} \
     @addField(Foo) private var injected : int; \
     @addMethod(Foo) function R() { var f : Foo; f.injected = 1; }\n"
)]
#[case::protected_member(
    "class A { protected var visible : int; } function F() { var a : A; a.visible = 1; }\n"
)]
#[case::known_function("function Helper() {} function F() { Helper(); }\n")]
#[case::this_shorthand_method_call(
    "class A { function Helper() {} function Run() { Helper(); } }\n"
)]
#[case::this_shorthand_inherited_method_call(
    "//- /a.ws\nclass Base { function Helper() {} }\n\
     //- /b.ws\nclass Child extends Base { function Run() { Helper(); } }\n"
)]
#[case::known_local("function F() { var x : int; var y : int; y = x; }\n")]
#[case::known_parameter("function F(p : int) { var y : int; y = p; }\n")]
#[case::this_shorthand_field(
    "class A { var known : int; function R() { var y : int; y = known; } }\n"
)]
#[case::parent_state_owner_member(
    "statemachine class Owner { function Help() {} } \
     state St in Owner { entry function Run() { parent.Help(); } }\n"
)]
#[case::state_method_inherited_through_extends_chain(
    "statemachine class Owner {} \
     state Base in Owner { function Help() {} } \
     state Mid in Owner extends Base {} \
     state Leaf in Owner extends Mid { entry function Run() { Help(); } }\n"
)]
#[case::wrapped_method_inside_wrap_method(
    "class Foo {} \
     @wrapMethod(Foo) function W() { wrappedMethod(); }\n"
)]
#[case::enum_member_used_as_value("enum E { V } function F() { var x : E; x = V; }\n")]
#[case::type_name_in_annotation("class C {} function F() { var x : C; }\n")]
#[case::type_name_in_new_expr(
    "class C {} function F(host : C) { var c : C; c = new C in host; }\n"
)]
#[case::type_name_in_cast(
    "class C {} class D {} function F() { var c : C; var d : D; d = (D) c; }\n"
)]
#[case::struct_constructor_call(
    "struct Vector { var X, Y, Z : float; } \
     function F() { var v : Vector = Vector(0, 0, 0); }\n"
)]
#[case::function_called_when_same_name_state_exists(
    "statemachine class C {} \
     state Sleep in C {} \
     function Sleep() {} \
     function F() { Sleep(); }\n"
)]
#[case::function_called_when_same_name_state_extends_another(
    "statemachine class Owner {} \
     state BaseState in Owner {} \
     state Child in Owner extends BaseState {} \
     function BaseState() {} \
     function F() { BaseState(); }\n"
)]
#[case::add_method_on_state_not_flagged(
    "statemachine class C {} \
     state Sleep in C {} \
     @addMethod(Sleep) function Extra() {}\n"
)]
#[case::wrap_method_on_state_not_flagged(
    "statemachine class C {} \
     state Sleep in C {} \
     @wrapMethod(Sleep) function Extra() {}\n"
)]
#[case::synthetic_state_class_in_annotation(
    "statemachine class C {} \
     state Sleep in C {} \
     function F() { var s : CStateSleep; }\n"
)]
fn produces_no_diagnostics(#[case] fixture: &str) {
    let t = TestDb::new(fixture);
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "no diagnostics expected, got {result:?}");
}

#[rstest]
#[case::unknown_type_in_extends(
    "class Foo extends NoSuch {}\n",
    &["unknown_type"],
    &["NoSuch"],
)]
#[case::unknown_type_in_state_parent(
    "state Drive in NoSuch { }\n",
    &["unknown_type"],
    &["NoSuch"],
)]
#[case::unknown_type_in_var_annot(
    "function F() { var x : NoSuch; }\n",
    &["unknown_type"],
    &["NoSuch"],
)]
#[case::unknown_type_in_annotation_arg(
    "@addMethod(NoSuch) function Extra() {}\n",
    &["unknown_type"],
    &["NoSuch"],
)]
#[case::unknown_type_in_cast(
    "class A {} function F() { var a : A; var b : A; b = (NoSuch) a; }\n",
    &["unknown_type"],
    &["NoSuch"],
)]
#[case::unknown_member_on_known_receiver(
    "class A { var known : int; } function F() { var a : A; a.bogus = 1; }\n",
    &["unknown_member"],
    &["bogus", "'A'"],
)]
#[case::unknown_member_default_val(
    "class A { var known : int; default bogus = 1; }\n",
    &["unknown_member"],
    &["bogus"],
)]
#[case::default_auto_state_in_plain_class(
    "class Player { default autoState = 'Exploration'; }\n",
    &["unknown_member"],
    &["autoState"],
)]
#[case::default_for_unknown_member_in_unrelated_class(
    "class Plain { default missing = 'CR4Task'; }\n",
    &["unknown_member"],
    &["missing"],
)]
#[case::unknown_member_default_block(
    "class A { var known : int; defaults { bogus = 1; } }\n",
    &["unknown_member"],
    &["bogus"],
)]
#[case::unknown_function_bare_call(
    "function F() { Bogus(); }\n",
    &["unknown_function"],
    &["Bogus"],
)]
#[case::unknown_identifier_bare(
    "function F() { var x : int; x = bogus; }\n",
    &["unknown_identifier"],
    &["bogus"],
)]
#[case::wrapped_method_outside_wrap_method(
    "function F() { wrappedMethod(); }\n",
    &["unknown_function"],
    &["wrappedMethod"],
)]
#[case::wrapped_method_in_add_method(
    "class Foo {} \
     @addMethod(Foo) function A() { wrappedMethod(); }\n",
    &["unknown_function"],
    &["wrappedMethod"],
)]
#[case::type_used_as_value_enum_arg(
    "enum E { V } function EnumGetMin(e : int) : int { return 0; } \
     function F() { var i : int; i = EnumGetMin(E); }\n",
    &["type_used_as_value"],
    &["'E'"],
)]
#[case::type_used_as_value_class_arg(
    "class C {} function F(p : int) {} function G() { F(C); }\n",
    &["type_used_as_value"],
    &["'C'"],
)]
#[case::type_used_as_value_assignment(
    "class C {} function F() { var x : int; x = C; }\n",
    &["type_used_as_value"],
    &["'C'"],
)]
#[case::type_used_as_value_struct_assignment(
    "struct S { var x : int; } function F() { var v : int; v = S; }\n",
    &["type_used_as_value"],
    &["'S'"],
)]
#[case::type_used_as_function_call(
    "enum E { V } function F() { E(); }\n",
    &["type_used_as_value"],
    &["cannot be called", "'E'"],
)]
#[case::call_to_state_only_is_unknown_function(
    "statemachine class C {} \
     state Sleep in C {} \
     function F() { Sleep(); }\n",
    &["unknown_function"],
    &["Sleep"],
)]
#[case::state_only_used_in_type_position_is_unknown_type(
    "statemachine class C {} \
     state SomeState in C {} \
     function F() { var x : SomeState; }\n",
    &["unknown_type"],
    &["SomeState"],
)]
#[case::state_extends_unknown_base(
    "statemachine class Owner {} \
     state Child in Owner extends Missing {}\n",
    &["unknown_type"],
    &["Missing"],
)]
fn produces_kinds_and_message_substrings(
    #[case] fixture: &str,
    #[case] expected_kinds: &[&str],
    #[case] expected_substrings: &[&str],
) {
    let t = TestDb::new(fixture);
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(kinds(diags), expected_kinds, "kinds mismatch");
    for s in expected_substrings {
        assert!(
            diags[0].message.contains(s),
            "expected message to contain {s:?}, got {:?}",
            diags[0].message
        );
    }
}

#[rstest]
#[case::from_outside_class(
    "class A { private var hidden : int; } function F() { var a : A; a.hidden = 1; }\n",
    &["hidden", "'A'"],
)]
#[case::from_sibling_class(
    "class A { private var hidden : int; } \
     class B { function R() { var a : A; a.hidden = 1; } }\n",
    &["hidden", "'A'"],
)]
#[case::from_subclass(
    "class Super { private var hidden : int; } \
     class Sub extends Super { function R() { var s : Sub; s.hidden = 1; } }\n",
    &["hidden", "'Super'"],
)]
#[case::add_field_private_from_outside(
    "class Foo {} \
     @addField(Foo) private var injected : int; \
     function F() { var f : Foo; f.injected = 1; }\n",
    &["'Foo'"],
)]
fn private_member_flagged(#[case] fixture: &str, #[case] expected_substrings: &[&str]) {
    let t = TestDb::new(fixture);
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).unwrap();
    assert_eq!(
        kinds(diags),
        vec!["private_member_access"],
        "kinds mismatch"
    );
    for s in expected_substrings {
        assert!(
            diags[0].message.contains(s),
            "expected message to contain {s:?}, got {:?}",
            diags[0].message
        );
    }
}

#[test]
fn unknown_type_in_new_expr() {
    let t = TestDb::new(
        "class Owner {} function F() { var o : Owner; var x : Owner; x = new NoSuch in o; }\n",
    );
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).unwrap();
    assert!(
        kinds(diags).contains(&"unknown_type"),
        "expected unknown_type, got {:?}",
        kinds(diags)
    );
}

#[test]
fn unknown_member_hint_is_info_level() {
    let t = TestDb::new("class A { var known : int; hint bogus = \"tip\"; }\n");
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).unwrap();
    assert_eq!(kinds(diags), vec!["unknown_member"]);
    assert!(diags[0].message.contains("bogus"));
    assert_eq!(diags[0].severity, super::Severity::Info);
}

#[test]
fn cascading_unknown_receiver_skips_member() {
    let t = TestDb::new("function F(x : NoSuch) { x.field = 1; }\n");
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).unwrap();
    let codes = kinds(diags);
    assert!(
        codes.contains(&"unknown_type"),
        "expected unknown_type for NoSuch, got {codes:?}"
    );
    assert!(
        !codes.contains(&"unknown_member"),
        "should not flag .field when receiver type unknown, got {codes:?}"
    );
}

#[test]
fn primitive_receiver_skipped() {
    let t = TestDb::new("function F() { var n : int; n.field = 1; }\n");
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    let codes = result
        .get(t.primary_uri())
        .map(|d| kinds(d))
        .unwrap_or_default();
    assert!(
        !codes.contains(&"unknown_member"),
        "should not flag .field on primitive, got {codes:?}"
    );
}

#[test]
fn method_call_not_duplicated_as_member() {
    let t = TestDb::new("class A {} function F() { var a : A; a.Bogus(); }\n");
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    let diags = result
        .get(t.primary_uri())
        .map(|d| kinds(d))
        .unwrap_or_default();
    assert!(
        !diags.contains(&"unknown_member"),
        "should defer method call to unknown_method, got {diags:?}"
    );
}

#[test]
fn array_generic_produces_noise() {
    let t = TestDb::new("class A {} function F() { var xs : array<A>; }\n");
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).unwrap();
    let codes = kinds(diags);
    assert!(
        codes.contains(&"unknown_type"),
        "expected unknown_type on 'array' (acknowledged noise), got {codes:?}"
    );
}

#[test]
fn no_noise_inside_error_subtree() {
    let t = TestDb::new("function F() { x +=== bogus = ; }\n");
    let _ = collect_unknown_symbol_diagnostics(&t.search_docs(), &t.db());
}

#[test]
fn script_global_member_access_no_type_used_as_value() {
    let t = TestDb::new("function F() { theGame.GetPlayer(); }\n").with_base_doc(
        "file:///r4game.ws",
        "class CR4Game { public function GetPlayer() : CR4Player {} }\n",
    );
    let env = script_env("theGame", "CR4Game");
    let db = t.db().with_script_env(&env);
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &db);
    assert!(
        result.is_empty(),
        "script global receiver should not trigger type_used_as_value, got {result:?}"
    );
}

#[test]
fn script_global_bare_use_no_type_used_as_value() {
    let t = TestDb::new("function F(p : CR4Game) { F(theGame); }\n")
        .with_base_doc("file:///r4game.ws", "class CR4Game {}\n");
    let env = script_env("theGame", "CR4Game");
    let db = t.db().with_script_env(&env);
    let result = collect_unknown_symbol_diagnostics(&t.search_docs(), &db);
    assert!(
        result.is_empty(),
        "script global passed as value should not trigger type_used_as_value, got {result:?}"
    );
}
