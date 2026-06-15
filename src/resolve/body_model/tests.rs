use rstest::rstest;

use super::BodyModel;
use crate::test_support::TestDb;

fn read_texts(src: &str) -> Vec<String> {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let byte = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(doc, byte).unwrap();
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

#[test]
fn reads_bucket_by_resolved_local_not_text() {
    let src = "function f() {\n    var $0tmp : int = 0;\n    Use(tmp);\n    var tmp : int = 1;\n    Use(tmp);\n    Use(tmp);\n}\n";
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let cursor = doc.line_index.position_to_byte(&doc.source, pos).unwrap();
    let model = BodyModel::enclosing(doc, cursor).unwrap();

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
