use super::{KIND, collect_inherited_field_diagnostics};
use crate::diagnostics::Severity;
use crate::test_support::TestDb;

#[test]
fn reports_field_redeclared_from_direct_base() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Base {\n  var hp : int;\n}\n",
        "//- /b.ws\n",
        "class Child extends Base {\n  var hp : int;\n}\n",
    ));

    let result = collect_inherited_field_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "exactly one inherited-field diagnostic");
    assert_eq!(diags[0].kind, KIND, "diagnostic kind");
    assert_eq!(diags[0].severity, Severity::Error, "error severity");
    assert_eq!(
        diags[0].message, "Field 'hp' is already declared in ancestor class 'Base'",
        "message names the field and ancestor",
    );
    assert_eq!(diags[0].related.len(), 1, "one related location");
    assert_eq!(
        diags[0].related[0].uri, "file:///a.ws",
        "related points at the ancestor declaration",
    );
}

#[test]
fn reports_field_redeclared_from_grandparent() {
    let t = TestDb::new(concat!(
        "class Grand {\n  var hp : int;\n}\n",
        "class Mid extends Grand {}\n",
        "class Child extends Mid {\n  var hp : int;\n}\n",
    ));

    let result = collect_inherited_field_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "grandparent fields count");
    assert_eq!(
        diags[0].message, "Field 'hp' is already declared in ancestor class 'Grand'",
        "message names the grandparent",
    );
}

#[test]
fn reports_field_declared_in_base_script() {
    let t = TestDb::new("class MyPlayer extends CR4Player {\n  var inv : int;\n}\n").with_base_doc(
        "file:///base/player.ws",
        "class CR4Player {\n  var inv : int;\n}\n",
    );

    let result = collect_inherited_field_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "base-script ancestor fields count");
    assert_eq!(
        diags[0].related[0].uri, "file:///base/player.ws",
        "related points at the base script",
    );
}

#[test]
fn reports_state_field_redeclared_from_base_state() {
    let t = TestDb::new(concat!(
        "statemachine class M {}\n",
        "state Base in M {\n  var hp : int;\n}\n",
        "state Child in M extends Base {\n  var hp : int;\n}\n",
    ));

    let result = collect_inherited_field_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "base-state fields count");
    assert_eq!(
        diags[0].message, "Field 'hp' is already declared in ancestor class 'Base'",
        "message names the base state",
    );
}

#[test]
fn accepts_field_matching_an_ancestor_method_name() {
    let t = TestDb::new(concat!(
        "class Base {\n  function Run() {}\n}\n",
        "class Child extends Base {\n  var Run : int;\n}\n",
    ));

    assert!(
        collect_inherited_field_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "only field-over-field redeclarations are this rule's concern",
    );
}

#[test]
fn accepts_unrelated_field_names() {
    let t = TestDb::new(concat!(
        "class Base {\n  var hp : int;\n}\n",
        "class Child extends Base {\n  var mp : int;\n}\n",
    ));

    assert!(
        collect_inherited_field_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "distinct field names are fine",
    );
}

#[test]
fn accepts_add_field_annotated_declaration() {
    let t = TestDb::new(concat!(
        "class Base {\n  var hp : int;\n}\n",
        "class Child extends Base {}\n",
        "@addField(Child) var hp : int;\n",
    ));

    assert!(
        collect_inherited_field_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "@addField declarations are out of this rule's scope",
    );
}

#[test]
fn accepts_struct_fields() {
    let t = TestDb::new("struct S {\n  var hp : int;\n}\n");

    assert!(
        collect_inherited_field_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "structs have no inheritance",
    );
}
