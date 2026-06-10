use rstest::rstest;

use super::super::{HighlightKind, document_highlights};
use crate::line_index::SourceRange;
use crate::test_support::TestDb;

use HighlightKind::{Read, Write};

fn kinds_at_cursor(fixture: &str) -> Vec<HighlightKind> {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let hits = document_highlights(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("symbol should resolve at cursor");
    hits.iter().map(|(_, kind)| *kind).collect()
}

#[rstest]
#[case::local_decl_assign_write_use_read(
    "function F() {\n var x : int;\n $0x = 1;\n Use(x);\n}\n",
    &[Write, Write, Read],
)]
#[case::compound_assignment_lhs_is_write(
    "function F() {\n var x : int;\n $0x += 1;\n}\n",
    &[Write, Write],
)]
#[case::field_member_assignment_is_write(
    "class C {\n var health : int;\n function F() {\n  this.$0health = 5;\n }\n}\n",
    &[Write, Write],
)]
#[case::object_of_member_assignment_is_read(
    "function F() {\n var a : int;\n $0a.b = 5;\n}\n",
    &[Write, Read],
)]
#[case::array_target_is_write(
    "function F() {\n var arr : array<int>;\n var i : int;\n $0arr[i] = 5;\n}\n",
    &[Write, Write],
)]
#[case::array_index_is_read(
    "function F() {\n var arr : array<int>;\n var $0i : int;\n arr[i] = 5;\n}\n",
    &[Write, Read],
)]
#[case::parameter_declaration_is_write_use_read(
    "function F($0p : int) {\n Use(p);\n}\n",
    &[Write, Read],
)]
#[case::function_name_occurrences_all_read(
    "function $0Foo() {}\nfunction Bar() {\n Foo();\n Foo();\n}\n",
    &[Read, Read, Read],
)]
fn classifies_occurrence_kinds(#[case] fixture: &str, #[case] expected: &[HighlightKind]) {
    assert_eq!(kinds_at_cursor(fixture), expected, "fixture: {fixture:?}");
}

#[test]
fn default_member_assignment_is_write() {
    let kinds = kinds_at_cursor("class C {\n var x : int;\n default $0x = 5;\n}\n");
    assert!(
        kinds.contains(&Write),
        "default-value target should be Write, got {kinds:?}"
    );
}

#[test]
fn returns_none_at_whitespace() {
    let t = TestDb::new("function F() {\n  $0\n}\n");
    let (uri, pos) = t.cursor();
    assert!(
        document_highlights(&uri, t.doc_for(&uri), &t.db(), pos).is_none(),
        "no symbol at whitespace should yield None"
    );
}

#[test]
fn declaration_in_other_file_is_filtered_out() {
    let t = TestDb::new(concat!(
        "//- /base.ws\n",
        "class CPlayer {\n  public function OnSpawned() {}\n}\n",
        "//- /a.ws\n",
        "function Caller() {\n  var p : CPlayer;\n  p.On$0Spawned();\n}\n",
    ));
    let (uri, pos) = t.cursor();
    let hits: Vec<(SourceRange, HighlightKind)> =
        document_highlights(&uri, t.doc_for(&uri), &t.db(), pos).expect("call site resolves");
    assert_eq!(
        hits.iter().map(|(_, k)| *k).collect::<Vec<_>>(),
        vec![Read],
        "only the in-file call site is highlighted; base-script declaration is dropped"
    );
}
