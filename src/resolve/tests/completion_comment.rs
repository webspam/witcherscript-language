use rstest::rstest;

use super::super::position_in_comment;
use crate::test_support::TestDb;

#[rstest]
#[case::line_comment("function f() {\n  // hello $0\n}\n", true)]
#[case::line_comment_ending_in_dot("function f() {\n  // pick up the loot.$0\n}\n", true)]
#[case::block_comment("function f() {\n  /* hello $0 */\n}\n", true)]
#[case::code_statement("function f() {\n  var x : int;\n  x$0\n}\n", false)]
#[case::dot_before_trailing_comment("function f() {\n  this.$0// note\n}\n", false)]
#[case::before_a_line_comment("function f() {\n  $0// note\n}\n", false)]
fn position_in_comment_detects_comment_context(#[case] source: &str, #[case] expected: bool) {
    let t = TestDb::new(source);
    let (uri, pos) = t.cursor();
    let got = position_in_comment(t.doc_for(&uri), pos);
    assert_eq!(got, expected, "case source: {source:?}");
}
