use rstest::rstest;

use crate::resolve::extract_common::apply_splices;
use crate::resolve::{BodyModel, Confidence, inline_variable};
use crate::test_support::TestDb;

fn inline_outcome(src: &str) -> Option<(String, bool)> {
    let t = TestDb::new(src);
    let (uri, pos) = t.cursor();
    let doc = t.doc_for(&uri);
    let byte = doc.line_index.position_to_byte(&doc.source, pos)?;
    let db = t.db();
    let model = BodyModel::enclosing(&uri, doc, &db, byte)?;
    let inlining = inline_variable(&model, byte)?;
    let verified = matches!(inlining.plan.confidence, Confidence::Verified);
    Some((apply_splices(&doc.source, &inlining.plan.edits), verified))
}

fn inlined(src: &str) -> Option<String> {
    inline_outcome(src).map(|(text, _)| text)
}

#[rstest]
#[case::all_usages_from_declaration(
    "all usages from declaration",
    "function f() {\n    var $0count : int = 5;\n    Foo(count);\n    Bar(count);\n}\n",
    "function f() {\n    Foo(5);\n    Bar(5);\n}\n"
)]
#[case::all_usages_from_name_end(
    "all usages from end of declaration name",
    "function f() {\n    var count$0 : int = 5;\n    Foo(count);\n    Bar(count);\n}\n",
    "function f() {\n    Foo(5);\n    Bar(5);\n}\n"
)]
#[case::all_usages_from_var_keyword(
    "all usages from var keyword",
    "function f() {\n    va$0r count : int = 5;\n    Foo(count);\n    Bar(count);\n}\n",
    "function f() {\n    Foo(5);\n    Bar(5);\n}\n"
)]
#[case::all_usages_before_var_keyword(
    "all usages before var keyword",
    "function f() {\n    $0var count : int = 5;\n    Foo(count);\n    Bar(count);\n}\n",
    "function f() {\n    Foo(5);\n    Bar(5);\n}\n"
)]
#[case::single_usage_from_use(
    "single usage from use",
    "function f() {\n    var count : int = 5;\n    Foo($0count);\n    Bar(count);\n}\n",
    "function f() {\n    var count : int = 5;\n    Foo(5);\n    Bar(count);\n}\n"
)]
#[case::wraps_compound_initializer(
    "wraps compound initializer",
    "function f(a : int, b : int) {\n    var $0sum : int = a + b;\n    return sum * 2;\n}\n",
    "function f(a : int, b : int) {\n    return (a + b) * 2;\n}\n"
)]
#[case::no_parens_as_return_value(
    "a compound value is bare when it is the whole return value",
    "function f(a : int, b : int) {\n    var $0sum : int = a + b;\n    return sum;\n}\n",
    "function f(a : int, b : int) {\n    return a + b;\n}\n"
)]
#[case::no_parens_as_argument(
    "a compound value is bare when it is a whole argument",
    "function f(a : int, b : int) {\n    var $0sum : int = a + b;\n    Foo(sum);\n}\n",
    "function f(a : int, b : int) {\n    Foo(a + b);\n}\n"
)]
#[case::field_with_same_name_untouched(
    "field with same name untouched",
    "class C {\n    var count : int;\n    function f() {\n        var $0count : int = 5;\n        Foo(count);\n        Foo(this.count);\n    }\n}\n",
    "class C {\n    var count : int;\n    function f() {\n        Foo(5);\n        Foo(this.count);\n    }\n}\n"
)]
#[case::last_use_deletes_declaration(
    "last use deletes declaration",
    "function f() {\n    var count : int = 5;\n    return $0count;\n}\n",
    "function f() {\n    return 5;\n}\n"
)]
#[case::assign_later_from_use(
    "assign later, inline from use",
    "function f() {\n    var x : int;\n    x = 13;\n    return $0x;\n}\n",
    "function f() {\n    return 13;\n}\n"
)]
#[case::assign_later_from_declaration(
    "assign later, inline from declaration",
    "function f() {\n    var $0x : int;\n    x = 13;\n    return x;\n}\n",
    "function f() {\n    return 13;\n}\n"
)]
#[case::assign_later_all_usages(
    "assign later, multiple usages",
    "function f() {\n    var $0x : int;\n    x = 13;\n    Foo(x);\n    Bar(x);\n}\n",
    "function f() {\n    Foo(13);\n    Bar(13);\n}\n"
)]
#[case::assign_later_single_usage_keeps_rest(
    "assign later, one of several usages",
    "function f() {\n    var x : int;\n    x = 13;\n    Foo($0x);\n    Bar(x);\n}\n",
    "function f() {\n    var x : int;\n    x = 13;\n    Foo(13);\n    Bar(x);\n}\n"
)]
#[case::wraps_compound_assignment(
    "assign later, compound value wrapped",
    "function f(a : int, b : int) {\n    var $0sum : int;\n    sum = a + b;\n    return sum * 2;\n}\n",
    "function f(a : int, b : int) {\n    return (a + b) * 2;\n}\n"
)]
#[case::multi_name_inline_first(
    "multi-name list, inline the first name",
    "function f() {\n    var marker, line : string;\n    marker = \"x\";\n    line = \"y\";\n    Foo($0marker);\n    Bar(line);\n}\n",
    "function f() {\n    var line : string;\n    line = \"y\";\n    Foo(\"x\");\n    Bar(line);\n}\n"
)]
#[case::multi_name_inline_last(
    "multi-name list, inline a later name",
    "function f() {\n    var marker, line : string;\n    marker = \"x\";\n    line = \"y\";\n    Foo(marker);\n    Bar($0line);\n}\n",
    "function f() {\n    var marker : string;\n    marker = \"x\";\n    Foo(marker);\n    Bar(\"y\");\n}\n"
)]
#[case::reassigned_after_init(
    "reassignment overwrites the initializer",
    "function f() {\n    var $0x : int = 5;\n    x = 10;\n    Foo(x);\n}\n",
    "function f() {\n    Foo(10);\n}\n"
)]
#[case::last_of_two_assignments(
    "the last assignment reaches the read",
    "function f() {\n    var x : int;\n    x = 1;\n    x = 2;\n    return $0x;\n}\n",
    "function f() {\n    return 2;\n}\n"
)]
#[case::dead_initializer_overwritten(
    "dead initializer overwritten before any read",
    "function f() {\n    var $0rah : int = 0;\n    rah = 14;\n    if (true) {\n        return rah;\n    }\n}\n",
    "function f() {\n    if (true) {\n        return 14;\n    }\n}\n"
)]
#[case::do_while_def_reaches_after(
    "assignment in a do-while body reaches a read after the loop",
    "function f() {\n    var x : int;\n    do {\n        x = 5;\n    } while (c);\n    return $0x;\n}\n",
    "function f() {\n    do {\n    } while (c);\n    return 5;\n}\n"
)]
#[case::switch_single_def_dominates(
    "a single definition reaches a read inside a switch case",
    "function f() {\n    var x : int = 9;\n    switch (s) {\n    case 1:\n        Foo($0x);\n        break;\n    }\n}\n",
    "function f() {\n    switch (s) {\n    case 1:\n        Foo(9);\n        break;\n    }\n}\n"
)]
#[case::operand_mutated_before_def(
    "a receiver mutation before the definition does not block the operand",
    "function f(target : CObj) {\n    var kind : name;\n    target.Prepare();\n    kind = GetKind(target);\n    if (!Check($0kind)) return false;\n}\n",
    "function f(target : CObj) {\n    target.Prepare();\n    if (!Check(GetKind(target))) return false;\n}\n"
)]
#[case::single_new_relocates(
    "a single new is a pure relocation",
    "class C {\n    function f() {\n        var $0a : C = new C in this;\n        return a;\n    }\n}\n",
    "class C {\n    function f() {\n        return new C in this;\n    }\n}\n"
)]
fn inlines(#[case] label: &str, #[case] src: &str, #[case] expected: &str) {
    let (got, verified) =
        inline_outcome(src).unwrap_or_else(|| panic!("case {label}: expected an inlining"));
    assert_eq!(got, expected, "case {label}: inlined output mismatch");
    assert!(verified, "case {label}: expected a verified inline");
}

#[rstest]
#[case::no_initializer(
    "no initializer",
    "function f() {\n    var $0x : int;\n    Foo(x);\n}\n"
)]
#[case::multi_name_declaration(
    "multi-name declaration",
    "function f() {\n    var $0a, b : int = 0;\n    Foo(a);\n}\n"
)]
#[case::single_usage_on_write_target(
    "single usage on write target",
    "function f() {\n    var x : int = 5;\n    $0x = 10;\n    Foo(x);\n}\n"
)]
#[case::read_before_assignment(
    "read precedes the only assignment",
    "function f() {\n    var x : int;\n    Foo($0x);\n    x = 13;\n}\n"
)]
#[case::conditional_assignment(
    "read reached by a conditional assignment and the bare declaration",
    "function f() {\n    var x : int;\n    if (c) { x = 13; }\n    return $0x;\n}\n"
)]
#[case::two_distinct_defs_reach_read(
    "two different assignments reach the read",
    "function f() {\n    var x : int;\n    if (c) { x = 1; } else { x = 2; }\n    return $0x;\n}\n"
)]
#[case::self_referential_def(
    "the value reads the variable being inlined",
    "function f() {\n    var x : int = 0;\n    x = x + 1;\n    return $0x;\n}\n"
)]
#[case::loop_back_edge_two_defs(
    "a read inside a loop is reached by two iterations' definitions",
    "function f() {\n    var x : int = 0;\n    while (c) {\n        Foo($0x);\n        x = 1;\n    }\n}\n"
)]
#[case::out_arg_unknown_value(
    "an out-argument gives the variable an unknown value",
    "function GetThing(out v : int) {}\nfunction f() {\n    var x : int;\n    GetThing(out x);\n    return $0x;\n}\n"
)]
#[case::compound_assignment_value(
    "a compound assignment depends on the prior value",
    "function f() {\n    var x : int = 1;\n    x += 2;\n    return $0x;\n}\n"
)]
#[case::switch_fallthrough_two_defs(
    "fallthrough makes two definitions reach the read",
    "function f() {\n    var x : int = 0;\n    switch (s) {\n    case 1:\n        x = 1;\n    case 2:\n        Foo($0x);\n        break;\n    }\n}\n"
)]
#[case::no_usages(
    "declaration with no usages",
    "function f() {\n    var $0x : int = 1;\n}\n"
)]
fn refuses(#[case] label: &str, #[case] src: &str) {
    assert!(
        inlined(src).is_none(),
        "case {label}: expected no inlining offered"
    );
}

#[rstest]
#[case::operand_clobbered(
    "an operand of the value is reassigned before the read",
    "function f() {\n    var a : int = 1;\n    var x : int = 0;\n    x = a;\n    a = 99;\n    return $0x;\n}\n",
    "function f() {\n    var a : int = 1;\n    a = 99;\n    return a;\n}\n"
)]
#[case::side_effecting_dead_store(
    "dropping a dead store removes its call",
    "function f() {\n    var x : int = 0;\n    x = Compute();\n    x = 14;\n    return $0x;\n}\n",
    "function f() {\n    return 14;\n}\n"
)]
#[case::unresolved_receiver(
    "the value calls a method on an unresolved receiver",
    "function f(groupId : int) {\n    var idx : int;\n    idx = config.GetGroupIdx(groupId);\n    return $0idx;\n}\n",
    "function f(groupId : int) {\n    return config.GetGroupIdx(groupId);\n}\n"
)]
#[case::call_duplicated_across_reads(
    "inlining a call into several reads runs it more than once",
    "function Compute() : int {\n    return 1;\n}\nfunction f() {\n    var $0x : int = Compute();\n    Foo(x);\n    Bar(x);\n}\n",
    "function Compute() : int {\n    return 1;\n}\nfunction f() {\n    Foo(Compute());\n    Bar(Compute());\n}\n"
)]
#[case::new_duplicated_across_reads(
    "inlining a new into several reads constructs more than one object",
    "class C {\n    function f() {\n        var $0a : C = new C in this;\n        Foo(a);\n        Bar(a);\n    }\n}\n",
    "class C {\n    function f() {\n        Foo(new C in this);\n        Bar(new C in this);\n    }\n}\n"
)]
fn offers_flagged(#[case] label: &str, #[case] src: &str, #[case] expected: &str) {
    let (got, verified) =
        inline_outcome(src).unwrap_or_else(|| panic!("case {label}: expected a flagged inlining"));
    assert_eq!(got, expected, "case {label}: inlined output mismatch");
    assert!(!verified, "case {label}: expected an unverified inline");
}
