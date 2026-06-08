use super::super::{OverrideBody, OverrideCompletion, override_completions};
use crate::test_support::{TestDb, def_names};

fn run(fixture: &str) -> Option<OverrideCompletion> {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    override_completions(t.doc_for(&uri), &t.db(), pos)
}

#[test]
fn offers_globals_with_function_keyword_after_space() {
    let result = run(concat!("function GlobalFunc() {}\n", "@replaceMethod $0\n"))
        .expect("globals should be offered directly after the annotation without `()`");

    assert!(
        result.needs_function_keyword,
        "insert must lead with `function` before the keyword is typed"
    );
    assert!(
        matches!(result.body, OverrideBody::Replace),
        "global override is always a replace"
    );
    assert!(
        def_names(&result.methods).contains(&"GlobalFunc"),
        "global function should be offered"
    );
}

#[test]
fn offers_globals_after_function_keyword() {
    let result = run(concat!(
        "function GlobalFunc() {}\n",
        "@replaceMethod\n",
        "function $0\n",
    ))
    .expect("globals should be offered after `function`");

    assert!(
        !result.needs_function_keyword,
        "`function` already typed; insert must not repeat it"
    );
    assert!(
        def_names(&result.methods).contains(&"GlobalFunc"),
        "global function should be offered"
    );
}

#[test]
fn offers_globals_while_typing_name() {
    let result = run(concat!(
        "function GlobalFunc() {}\n",
        "@replaceMethod Glo$0\n"
    ))
    .expect("globals should be offered while typing the name");

    assert!(
        def_names(&result.methods).contains(&"GlobalFunc"),
        "global function should be offered"
    );
}

#[test]
fn none_without_whitespace_gap() {
    assert!(
        run(concat!("function GlobalFunc() {}\n", "@replaceMethod$0\n")).is_none(),
        "no space after the annotation belongs to the snippet path, not globals"
    );
}

#[test]
fn none_for_wrap_method() {
    assert!(
        run(concat!("function GlobalFunc() {}\n", "@wrapMethod $0\n")).is_none(),
        "globals without `()` are a @replaceMethod-only feature"
    );
}

#[test]
fn excludes_exec_functions() {
    let result = run(concat!(
        "function GlobalFunc() {}\n",
        "exec function DebugCmd() {}\n",
        "@replaceMethod $0\n",
    ))
    .expect("globals should be offered");

    let names = def_names(&result.methods);
    assert!(names.contains(&"GlobalFunc"), "ordinary global must appear");
    assert!(
        !names.contains(&"DebugCmd"),
        "exec function must not be offered"
    );
}
