use super::{KIND_WEAKER_ACCESS, collect_override_consistency_diagnostics};
use crate::diagnostics::Severity;
use crate::test_support::TestDb;

#[test]
fn reports_private_override_of_public_base_method() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Base {\n  public function Run() {}\n}\n",
        "//- /b.ws\n",
        "class Child extends Base {\n  private function Run() {}\n}\n",
    ));

    let result = collect_override_consistency_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get("file:///b.ws").expect("b.ws should be flagged");
    assert_eq!(diags.len(), 1, "exactly one weaker-access diagnostic");
    assert_eq!(diags[0].kind, KIND_WEAKER_ACCESS, "diagnostic kind");
    assert_eq!(diags[0].severity, Severity::Error, "error severity");
    assert_eq!(
        diags[0].message,
        "Function 'Run' cannot have a weaker access modifier than in ancestor class 'Base'",
        "message mirrors the compiler error",
    );
    assert_eq!(
        diags[0].related[0].uri, "file:///a.ws",
        "related points at the base declaration",
    );
}

#[test]
fn reports_private_override_of_unmodified_base_method() {
    let t = TestDb::new(concat!(
        "class Base {\n  function Run() {}\n}\n",
        "class Child extends Base {\n  private function Run() {}\n}\n",
    ));

    let result = collect_override_consistency_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "an unmodified base method is public");
}

#[test]
fn reports_weaker_access_than_base_script_method() {
    let t = TestDb::new(concat!(
        "class MyPlayer extends CR4Player {\n",
        "  private function ShowToast() {}\n",
        "}\n",
    ))
    .with_base_doc(
        "file:///base/player.ws",
        "class CR4Player {\n  protected function ShowToast() {}\n}\n",
    );

    let result = collect_override_consistency_diagnostics(&t.search_docs(), &t.db());

    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "base-script methods count");
    assert_eq!(
        diags[0].message,
        "Function 'ShowToast' cannot have a weaker access modifier than in \
         ancestor class 'CR4Player'",
        "message names the base-script class",
    );
}

#[test]
fn accepts_same_access_override() {
    let t = TestDb::new(concat!(
        "class Base {\n  protected function Run() {}\n}\n",
        "class Child extends Base {\n  protected function Run() {}\n}\n",
    ));

    assert!(
        collect_override_consistency_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "matching access is fine",
    );
}

#[test]
fn accepts_stronger_access_override() {
    let t = TestDb::new(concat!(
        "class Base {\n  private function Run() {}\n}\n",
        "class Child extends Base {\n  protected function Run() {}\n}\n",
    ));

    assert!(
        collect_override_consistency_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "widening access is fine",
    );
}

#[test]
fn accepts_method_without_ancestor_counterpart() {
    let t = TestDb::new(concat!(
        "class Base {\n  public function Run() {}\n}\n",
        "class Child extends Base {\n  private function Walk() {}\n}\n",
    ));

    assert!(
        collect_override_consistency_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "a new method name is not an override",
    );
}

#[test]
fn accepts_wrap_method_annotated_function() {
    let t = TestDb::new(concat!(
        "class Base {\n  public function Run() {}\n}\n",
        "class Child extends Base {}\n",
        "@wrapMethod(Child) private function Run() { wrappedMethod(); }\n",
    ));

    assert!(
        collect_override_consistency_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "annotated functions are out of this rule's scope",
    );
}

#[test]
fn accepts_function_matching_a_base_event_name() {
    let t = TestDb::new(concat!(
        "class Base {\n  event OnHit() {\n  }\n}\n",
        "class Child extends Base {\n  private function OnHit() {}\n}\n",
    ));

    assert!(
        collect_override_consistency_diagnostics(&t.search_docs(), &t.db()).is_empty(),
        "a same-named base event is not a method override",
    );
}
