use super::collect_unknown_symbol_diagnostics;
use crate::diagnostics::collect_cst_diagnostics_for_document;
use crate::document::{parse_document, ParsedDocument};
use crate::resolve::{SymbolDb, WorkspaceIndex};

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
    let mut idx = WorkspaceIndex::default();
    let doc = parse_document(&src).expect("parse should succeed");
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

fn index_and_docs(docs: &[(&str, &str)]) -> (WorkspaceIndex, Vec<(String, ParsedDocument)>) {
    let mut idx = WorkspaceIndex::default();
    let mut parsed = Vec::new();
    for (uri, src) in docs {
        let doc = parse_document(*src).expect("parse should succeed");
        idx.update_document(*uri, &doc);
        parsed.push((uri.to_string(), doc));
    }
    (idx, parsed)
}

fn check(
    idx: &WorkspaceIndex,
    docs: &[(String, ParsedDocument)],
) -> std::collections::HashMap<String, Vec<super::WorkspaceDiagnostic>> {
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(idx, &base);
    let doc_pairs: Vec<(&str, &ParsedDocument)> =
        docs.iter().map(|(uri, doc)| (uri.as_str(), doc)).collect();
    collect_unknown_symbol_diagnostics(&doc_pairs, &db)
}

fn kinds(diags: &[super::WorkspaceDiagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.kind.as_str()).collect()
}

#[test]
fn declarations_do_not_fire() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         struct S {} \
         enum E { V } \
         function F(a, b : int) { var x, y : int; } \
         event Ev() {} \
         state St in Foo { entry function Run() {} }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "no diagnostics expected, got {result:?}");
}

#[test]
fn unknown_type_in_extends() {
    let (idx, docs) = index_and_docs(&[("file:///t.ws", "class Foo extends NoSuch {}\n")]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_type"]);
    assert!(diags[0].message.contains("NoSuch"));
}

#[test]
fn unknown_type_in_state_parent() {
    let (idx, docs) = index_and_docs(&[("file:///t.ws", "state Drive in NoSuch { }\n")]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_type"]);
    assert!(diags[0].message.contains("NoSuch"));
}

#[test]
fn unknown_type_in_var_annot() {
    let (idx, docs) = index_and_docs(&[("file:///t.ws", "function F() { var x : NoSuch; }\n")]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_type"]);
    assert!(diags[0].message.contains("NoSuch"));
}

#[test]
fn unknown_type_in_new_expr() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Owner {} function F() { var o : Owner; var x : Owner; x = new NoSuch in o; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert!(
        kinds(diags).contains(&"unknown_type"),
        "expected unknown_type, got {:?}",
        kinds(diags)
    );
}

#[test]
fn unknown_type_in_annotation_arg() {
    let (idx, docs) =
        index_and_docs(&[("file:///t.ws", "@addMethod(NoSuch) function Extra() {}\n")]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_type"]);
    assert!(diags[0].message.contains("NoSuch"));
}

#[test]
fn unknown_type_in_cast() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A {} function F() { var a : A; var b : A; b = (NoSuch) a; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_type"]);
    assert!(diags[0].message.contains("NoSuch"));
}

#[test]
fn builtin_types_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "function F(a : bool, b : int, c : float, d : string, e : name, f : byte) : void {}\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn builtin_type_aliases_skipped() {
    let cases: &[(&str, &str)] = &[
        ("Bool", "function F(a : Bool) {}\n"),
        ("Float", "function F(a : Float) {}\n"),
        ("String", "function F(a : String) {}\n"),
        ("CName", "function F(a : CName) {}\n"),
        ("Int32", "function F(a : Int32) {}\n"),
        ("Uint8", "function F(a : Uint8) {}\n"),
        ("Int16", "function F(a : Int16) {}\n"),
        ("Int8", "function F(a : Int8) {}\n"),
        ("Uint32", "function F(a : Uint32) {}\n"),
        ("Uint16", "function F(a : Uint16) {}\n"),
        ("StringAnsi", "function F(a : StringAnsi) {}\n"),
    ];
    for (label, src) in cases {
        let (idx, docs) = index_and_docs(&[("file:///t.ws", *src)]);
        let result = check(&idx, &docs);
        assert!(
            result.is_empty(),
            "case {label}: expected no diagnostics, got {result:?}",
        );
    }
}

#[test]
fn known_type_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A {} class B extends A { var a : A; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn unknown_member_on_known_receiver() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { var known : int; } function F() { var a : A; a.bogus = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_member"]);
    assert!(diags[0].message.contains("bogus"));
    assert!(diags[0].message.contains("'A'"));
}

#[test]
fn unknown_member_default_val() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { var known : int; default bogus = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_member"]);
    assert!(diags[0].message.contains("bogus"));
}

#[test]
fn default_auto_state_not_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "statemachine class Player { default autoState = 'Exploration'; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(
        result.is_empty(),
        "default autoState is a statemachine construct, not a member, got {result:?}"
    );
}

#[test]
fn default_auto_state_in_plain_class_still_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Player { default autoState = 'Exploration'; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_member"]);
    assert!(diags[0].message.contains("autoState"));
}

#[test]
fn default_on_private_inherited_field_not_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Super { private var hidden : int; default hidden = 1; } \
         class Sub extends Super { default hidden = 2; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(
        result.is_empty(),
        "subclass may override a private inherited default, got {result:?}"
    );
}

#[test]
fn hint_on_private_inherited_field_not_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Super { private var hidden : int; } \
         class Sub extends Super { hint hidden = \"tip\"; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(
        result.is_empty(),
        "subclass may set the hint of a private inherited field, got {result:?}"
    );
}

#[test]
fn default_for_unknown_member_in_unrelated_class_still_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Plain { default missing = 'CR4Task'; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_member"]);
    assert!(diags[0].message.contains("missing"));
}

#[test]
fn unknown_member_default_block() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { var known : int; defaults { bogus = 1; } }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_member"]);
    assert!(diags[0].message.contains("bogus"));
}

#[test]
fn unknown_member_hint_is_info_level() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { var known : int; hint bogus = \"tip\"; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_member"]);
    assert!(diags[0].message.contains("bogus"));
    assert_eq!(diags[0].severity, super::Severity::Info);
}

#[test]
fn known_member_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { var known : int; } function F() { var a : A; a.known = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn private_member_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { private var hidden : int; function R() { var a : A; a.hidden = 1; } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn private_member_flagged_from_outside_class() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { private var hidden : int; } function F() { var a : A; a.hidden = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["private_member_access"]);
    assert!(diags[0].message.contains("hidden"));
    assert!(diags[0].message.contains("'A'"));
}

#[test]
fn private_member_flagged_from_sibling_class() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { private var hidden : int; } \
         class B { function R() { var a : A; a.hidden = 1; } }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["private_member_access"]);
    assert!(diags[0].message.contains("hidden"));
    assert!(diags[0].message.contains("'A'"));
}

#[test]
fn private_member_flagged_from_subclass() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Super { private var hidden : int; } \
         class Sub extends Super { function R() { var s : Sub; s.hidden = 1; } }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["private_member_access"]);
    assert!(diags[0].message.contains("hidden"));
    assert!(diags[0].message.contains("'Super'"));
}

#[test]
fn private_member_not_flagged_inside_add_method_of_declaring_class() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { private var hidden : int; } \
         @addMethod(A) function R() { var a : A; a.hidden = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(
        result.is_empty(),
        "@addMethod(A) body is a member of A, got {result:?}"
    );
}

#[test]
fn add_field_private_accessible_inside_add_method_of_same_class() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @addField(Foo) private var injected : int; \
         @addMethod(Foo) function R() { var f : Foo; f.injected = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(
        result.is_empty(),
        "an @addField on Foo is a private member of Foo; access from @addMethod(Foo) is allowed, got {result:?}"
    );
}

#[test]
fn add_field_private_flagged_with_class_name_from_outside() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @addField(Foo) private var injected : int; \
         function F() { var f : Foo; f.injected = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["private_member_access"]);
    assert!(
        diags[0].message.contains("'Foo'"),
        "message must name the declaring class, got {:?}",
        diags[0].message
    );
}

#[test]
fn protected_member_not_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { protected var visible : int; } function F() { var a : A; a.visible = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn cascading_unknown_receiver_skips_member() {
    let (idx, docs) =
        index_and_docs(&[("file:///t.ws", "function F(x : NoSuch) { x.field = 1; }\n")]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
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
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "function F() { var n : int; n.field = 1; }\n",
    )]);
    let result = check(&idx, &docs);
    let codes = result
        .get("file:///t.ws")
        .map(|d| kinds(d))
        .unwrap_or_default();
    assert!(
        !codes.contains(&"unknown_member"),
        "should not flag .field on primitive, got {codes:?}"
    );
}

#[test]
fn unknown_function_bare_call() {
    let (idx, docs) = index_and_docs(&[("file:///t.ws", "function F() { Bogus(); }\n")]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_function"]);
    assert!(diags[0].message.contains("Bogus"));
}

#[test]
fn known_function_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "function Helper() {} function F() { Helper(); }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn this_shorthand_method_call_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { function Helper() {} function Run() { Helper(); } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn this_shorthand_inherited_method_call_skipped() {
    let (idx, docs) = index_and_docs(&[
        ("file:///a.ws", "class Base { function Helper() {} }\n"),
        (
            "file:///b.ws",
            "class Child extends Base { function Run() { Helper(); } }\n",
        ),
    ]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn unknown_identifier_bare() {
    let (idx, docs) =
        index_and_docs(&[("file:///t.ws", "function F() { var x : int; x = bogus; }\n")]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_identifier"]);
    assert!(diags[0].message.contains("bogus"));
}

#[test]
fn known_local_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "function F() { var x : int; var y : int; y = x; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn known_parameter_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "function F(p : int) { var y : int; y = p; }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn this_shorthand_field_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A { var known : int; function R() { var y : int; y = known; } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn method_call_not_duplicated_as_member() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A {} function F() { var a : A; a.Bogus(); }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result
        .get("file:///t.ws")
        .map(|d| kinds(d))
        .unwrap_or_default();
    assert!(
        !diags.contains(&"unknown_member"),
        "should defer method call to unknown_method, got {diags:?}"
    );
}

#[test]
fn parent_state_owner_member_skipped() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "statemachine class Owner { function Help() {} } \
         state St in Owner { entry function Run() { parent.Help(); } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(result.is_empty(), "got {result:?}");
}

#[test]
fn state_method_inherited_through_extends_chain() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "statemachine class Owner {} \
         state Base in Owner { function Help() {} } \
         state Mid in Owner extends Base {} \
         state Leaf in Owner extends Mid { entry function Run() { Help(); } }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(
        result.is_empty(),
        "unqualified call to a method inherited via state extends must not be flagged, got {result:?}"
    );
}

#[test]
fn array_generic_produces_noise() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class A {} function F() { var xs : array<A>; }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    let codes = kinds(diags);
    assert!(
        codes.contains(&"unknown_type"),
        "expected unknown_type on 'array' (acknowledged noise), got {codes:?}"
    );
}

#[test]
fn no_noise_inside_error_subtree() {
    let (idx, docs) = index_and_docs(&[("file:///t.ws", "function F() { x +=== bogus = ; }\n")]);
    let _ = check(&idx, &docs);
}

#[test]
fn wrapped_method_call_inside_wrap_method_not_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @wrapMethod(Foo) function W() { wrappedMethod(); }\n",
    )]);
    let result = check(&idx, &docs);
    assert!(
        result.is_empty(),
        "wrappedMethod inside @wrapMethod should not be flagged, got {result:?}"
    );
}

#[test]
fn wrapped_method_call_outside_wrap_method_still_flagged() {
    let (idx, docs) = index_and_docs(&[("file:///t.ws", "function F() { wrappedMethod(); }\n")]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_function"]);
    assert!(diags[0].message.contains("wrappedMethod"));
}

#[test]
fn wrapped_method_in_add_method_still_flagged() {
    let (idx, docs) = index_and_docs(&[(
        "file:///t.ws",
        "class Foo {} \
         @addMethod(Foo) function A() { wrappedMethod(); }\n",
    )]);
    let result = check(&idx, &docs);
    let diags = result.get("file:///t.ws").unwrap();
    assert_eq!(kinds(diags), vec!["unknown_function"]);
    assert!(diags[0].message.contains("wrappedMethod"));
}
