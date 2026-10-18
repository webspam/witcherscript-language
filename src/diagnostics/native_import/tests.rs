use rstest::rstest;

use super::{KIND_UNKNOWN_NATIVE, collect_native_import_diagnostics};
use crate::test_support::TestDb;

#[rstest]
#[case::global_function("import function AbsF(f : float) : float;\n")]
#[case::class_with_field(
    "import class C2dArray extends CResource {\n  import var headers : array<string>;\n}\n"
)]
#[case::method_declared_on_native_base(
    "import class CResource extends CObject {\n  import function Clone() : CObject;\n}\n"
)]
#[case::scripted_member_in_imported_class(
    "import class C2dArray extends CResource {\n  function Helper() {}\n}\n"
)]
#[case::native_enum("import enum EScriptQueryFlags {}\n")]
fn valid_imports_do_not_fire(#[case] fixture: &str) {
    let t = TestDb::new(fixture).with_builtins_index();
    let result = collect_native_import_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}

#[rstest]
#[case::global_function(
    "import function NotANative();\n",
    "No native function 'NotANative' to import"
)]
#[case::type_name(
    "import class NotANative {}\n",
    "No native type 'NotANative' to import"
)]
#[case::member(
    "import class C2dArray extends CResource {\n  import var missing : int;\n}\n",
    "No native member 'missing' on 'C2dArray' to import"
)]
fn unknown_imports_fire(#[case] fixture: &str, #[case] message: &str) {
    let t = TestDb::new(fixture).with_builtins_index();
    let result = collect_native_import_diagnostics(&t.search_docs(), &t.db());
    let diags = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].kind, KIND_UNKNOWN_NATIVE);
    assert_eq!(diags[0].message, message);
}

#[test]
fn skipped_without_builtins() {
    let t = TestDb::new("import function NotANative();\n");
    let result = collect_native_import_diagnostics(&t.search_docs(), &t.db());
    assert!(result.is_empty(), "expected no diagnostics, got {result:?}");
}
