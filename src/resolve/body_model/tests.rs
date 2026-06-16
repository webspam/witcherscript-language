use rstest::rstest;

use super::{BodyModel, Stability};
use crate::test_support::TestDb;

fn read_texts(src: &str) -> Vec<String> {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let db = t.db();
    let byte = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(&uri, doc, &db, byte).unwrap();
    let local = model.local_declared_at(byte).unwrap();
    model
        .reads(local)
        .iter()
        .map(|r| doc.source[r.clone()].to_string())
        .collect()
}

#[rstest]
#[case::plain_read(
    "single read is reported",
    "function f() {\n    var $0x : int = 0;\n    Use(x);\n}\n",
    vec!["x"]
)]
#[case::write_target_excluded(
    "a whole-value assignment target is not a read",
    "function f() {\n    var $0x : int = 0;\n    x = 1;\n    Use(x);\n}\n",
    vec!["x"]
)]
#[case::compound_is_read(
    "a compound assignment reads the prior value",
    "function f() {\n    var $0x : int = 0;\n    x += 1;\n}\n",
    vec!["x"]
)]
#[case::member_base_is_read(
    "a member-target base is read",
    "function f() {\n    var $0p : Foo;\n    p.field = 1;\n}\n",
    vec!["p"]
)]
#[case::no_reads(
    "only a whole-value write yields no reads",
    "function f() {\n    var $0x : int = 0;\n    x = 1;\n}\n",
    Vec::<&str>::new()
)]
fn reads_report_value_occurrences(
    #[case] label: &str,
    #[case] src: &str,
    #[case] expected: Vec<&str>,
) {
    let got = read_texts(src);
    let got: Vec<&str> = got.iter().map(String::as_str).collect();
    assert_eq!(got, expected, "case {label}: read occurrences mismatch");
}

fn written_in_body(src: &str) -> bool {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let db = t.db();
    let byte = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(&uri, doc, &db, byte).unwrap();
    let local = model.local_declared_at(byte).unwrap();
    model.is_written_in(local, &(byte..doc.source.len()))
}

#[rstest]
#[case::assignment("function f() {\n    var $0x : int = 0;\n    x = 1;\n}\n", true)]
#[case::compound("function f() {\n    var $0x : int = 0;\n    x += 1;\n}\n", true)]
#[case::read_only("function f() {\n    var $0x : int = 0;\n    Use(x);\n}\n", false)]
#[case::value_type_in_place(
    "function f() {\n    var $0a : array<int>;\n    a.PushBack(1);\n}\n",
    true
)]
fn is_written_in_reports_mutations(#[case] src: &str, #[case] expected: bool) {
    assert_eq!(written_in_body(src), expected);
}

fn joinable(src: &str) -> Option<(String, String)> {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let db = t.db();
    let byte = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(&uri, doc, &db, byte).unwrap();
    let local = model.local_declared_at(byte).unwrap();
    let target = model.joinable_assignment(local)?;
    Some((
        doc.source[target.value].to_string(),
        doc.source[target.stmt].to_string(),
    ))
}

#[rstest]
#[case::adjacent("function f() {\n    var $0x : int;\n    x = 1;\n}\n", Some(("1", "x = 1;")))]
#[case::across_pure_decl(
    "function f() {\n    var $0x : int;\n    var y : int;\n    x = 2;\n}\n",
    Some(("2", "x = 2;"))
)]
#[case::already_initialised("function f() {\n    var $0x : int = 0;\n    x = 1;\n}\n", None)]
#[case::read_between(
    "function f() {\n    var $0x : int;\n    Use(x);\n    x = 1;\n}\n",
    None
)]
#[case::operand_introduced(
    "function f() {\n    var $0x : int;\n    var y : int = 5;\n    x = y;\n}\n",
    None
)]
fn joinable_assignment_reports_first_safe_assignment(
    #[case] src: &str,
    #[case] expected: Option<(&str, &str)>,
) {
    let got = joinable(src);
    let got = got.as_ref().map(|(v, s)| (v.as_str(), s.as_str()));
    assert_eq!(got, expected);
}

fn live_after_selection(src: &str, select: &str) -> bool {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let db = t.db();
    let byte = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(&uri, doc, &db, byte).unwrap();
    let local = model.local_declared_at(byte).unwrap();
    let start = doc.source.find(select).unwrap();
    model.live_after(local, &(start..start + select.len()))
}

#[test]
fn live_after_true_when_read_follows_selection() {
    let src = "function f() {\n    var $0x : int = 0;\n    x = 1;\n    Use(x);\n}\n";
    assert!(live_after_selection(src, "x = 1;"));
}

#[test]
fn live_after_false_without_later_use() {
    let src = "function f() {\n    var $0x : int = 0;\n    Use(x);\n    x = 1;\n}\n";
    assert!(!live_after_selection(src, "x = 1;"));
}

fn entry_value_unread(src: &str, span_from: &str) -> bool {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let db = t.db();
    let byte = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(&uri, doc, &db, byte).unwrap();
    let local = model.local_declared_at(byte).unwrap();
    let block = doc.source.find('{').unwrap()..doc.source.rfind('}').unwrap() + 1;
    let span = doc.source.find(span_from).unwrap()..block.end;
    model.entry_value_unread_in(local, &span, &block)
}

#[test]
fn entry_value_unread_when_overwritten_before_read() {
    let src = "function f() {\n    var $0x : int = 0;\n    x = 5;\n    Use(x);\n}\n";
    assert!(entry_value_unread(src, "x = 5;"));
}

#[test]
fn entry_value_read_when_used_before_overwrite() {
    let src = "function f() {\n    var $0x : int = 0;\n    Use(x);\n    x = 5;\n}\n";
    assert!(!entry_value_unread(src, "Use(x);"));
}

fn value_stability(src: &str, value_text: &str) -> &'static str {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let db = t.db();
    let byte = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(&uri, doc, &db, byte).unwrap();
    let target = model.local_declared_at(byte).unwrap();
    let vstart = doc.source.find(value_text).expect("value present");
    let value = vstart..vstart + value_text.len();
    let captured_at = doc.source[..vstart].rfind('\n').map_or(0, |i| i + 1);
    match model.value_stability(&value, captured_at, target) {
        Stability::Stable => "stable",
        Stability::MayChange => "may-change",
        Stability::ReferencesTarget => "references-target",
    }
}

#[rstest]
#[case::call_receiver_is_not_a_write(
    "a method call's receiver is part of the value, not a reassignment",
    "class CEntity {}\nfunction f(e : CEntity) {\n    var $0x : array<int>;\n    x = e.GetComponents();\n    Use(x);\n}\n",
    "e.GetComponents()",
    "stable"
)]
#[case::operand_reassigned_after_capture(
    "an operand reassigned between capture and read changes the value",
    "function f() {\n    var seed : int = 1;\n    var $0x : int = 0;\n    x = seed + 1;\n    seed = 99;\n    Use(x);\n}\n",
    "seed + 1",
    "may-change"
)]
fn value_stability_ignores_in_value_writes(
    #[case] label: &str,
    #[case] src: &str,
    #[case] value_text: &str,
    #[case] expected: &str,
) {
    assert_eq!(
        value_stability(src, value_text),
        expected,
        "case {label}: stability mismatch"
    );
}

#[test]
fn reads_bucket_by_resolved_local_not_text() {
    let src = "function f() {\n    var $0tmp : int = 0;\n    Use(tmp);\n    var tmp : int = 1;\n    Use(tmp);\n    Use(tmp);\n}\n";
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let db = t.db();
    let cursor = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(&uri, doc, &db, cursor).unwrap();

    let outer = model.local_declared_at(cursor).unwrap();
    let inner_decl = doc.source.match_indices("tmp").nth(2).unwrap().0;
    let inner = model.local_declared_at(inner_decl).unwrap();

    assert_ne!(outer, inner, "the two declarations are distinct locals");
    assert_eq!(
        model.reads(outer).len(),
        1,
        "outer tmp is read once before the redeclaration"
    );
    assert_eq!(
        model.reads(inner).len(),
        2,
        "inner tmp is read twice after the redeclaration"
    );
}
