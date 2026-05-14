use super::super::{signature_help, SignatureHelpInfo};
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;

/// Runs `signature_help` with the cursor at the `|` marker in `source`.
fn help(source: &str) -> Option<SignatureHelpInfo> {
    help_with_colon(source, false)
}

fn help_with_colon(source: &str, compact_colon: bool) -> Option<SignatureHelpInfo> {
    let offset = source
        .find('|')
        .expect("source must contain a | cursor marker");
    let source = source.replacen('|', "", 1);
    let doc = make_doc(&source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let before = &source[..offset];
    let line = before.matches('\n').count() as u32;
    let character = before.rsplit('\n').next().unwrap_or("").chars().count() as u32;

    signature_help(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition { line, character },
        compact_colon,
    )
}

#[test]
fn reports_label_and_parameter_offsets() {
    let info = help(concat!(
        "function Find(name : string, range : float) : int {}\n",
        "function Test() {\n",
        "  Find(|);\n",
        "}\n",
    ))
    .expect("signature help in call");

    assert_eq!(info.label, "Find(name : string, range : float) : int");
    assert_eq!(info.parameters.len(), 2);
    assert_eq!(info.active_parameter, Some(0));

    let (s0, e0) = info.parameters[0];
    assert_eq!(&info.label[s0 as usize..e0 as usize], "name : string");
    let (s1, e1) = info.parameters[1];
    assert_eq!(&info.label[s1 as usize..e1 as usize], "range : float");
}

#[test]
fn active_parameter_advances_with_commas() {
    let source = concat!(
        "function Find(a : int, b : int, c : int) {}\n",
        "function Test() {\n",
        "  Find(1, |2, 3);\n",
        "}\n",
    );
    assert_eq!(help(source).unwrap().active_parameter, Some(1));

    let source = concat!(
        "function Find(a : int, b : int, c : int) {}\n",
        "function Test() {\n",
        "  Find(1, 2, |3);\n",
        "}\n",
    );
    assert_eq!(help(source).unwrap().active_parameter, Some(2));
}

#[test]
fn resolves_method_call() {
    let info = help(concat!(
        "class CPlayer {\n",
        "  function GetHealth(modifier : float) : int {}\n",
        "}\n",
        "function Test() {\n",
        "  var p : CPlayer;\n",
        "  p.GetHealth(|);\n",
        "}\n",
    ))
    .expect("signature help for method call");

    assert_eq!(info.label, "GetHealth(modifier : float) : int");
}

#[test]
fn resolves_this_method_call() {
    let info = help(concat!(
        "class CPlayer {\n",
        "  function Helper(x : int) {}\n",
        "  function Run() {\n",
        "    this.Helper(|);\n",
        "  }\n",
        "}\n",
    ))
    .expect("signature help for this.method call");

    assert_eq!(info.label, "Helper(x : int)");
}

#[test]
fn zero_param_callee_has_no_active_parameter() {
    let info = help(concat!(
        "function NoArgs() {}\n",
        "function Test() {\n",
        "  NoArgs(|);\n",
        "}\n",
    ))
    .expect("signature help for zero-param call");

    assert_eq!(info.label, "NoArgs()");
    assert!(info.parameters.is_empty());
    assert_eq!(info.active_parameter, None);
}

#[test]
fn optional_and_out_params_appear_in_label() {
    let info = help(concat!(
        "function F(optional a : int, out b : float) {}\n",
        "function Test() {\n",
        "  F(1, |2);\n",
        "}\n",
    ))
    .expect("signature help with optional/out params");

    assert_eq!(info.label, "F(optional a : int, out b : float)");
    assert_eq!(info.parameters.len(), 2);
    assert_eq!(info.active_parameter, Some(1));
}

#[test]
fn multi_name_param_group_produces_separate_parameters() {
    let info = help(concat!(
        "function M(a, b : int) {}\n",
        "function Test() {\n",
        "  M(1, |2);\n",
        "}\n",
    ))
    .expect("signature help with multi-name param group");

    assert_eq!(info.parameters.len(), 2);
    assert_eq!(info.active_parameter, Some(1));
}

#[test]
fn nested_call_resolves_inner_callee() {
    let info = help(concat!(
        "function Inner(x : int) : int {}\n",
        "function Outer(y : int) : int {}\n",
        "function Test() {\n",
        "  Outer(Inner(|));\n",
        "}\n",
    ))
    .expect("signature help for nested call");

    assert_eq!(info.label, "Inner(x : int) : int");
}

#[test]
fn cursor_past_last_parameter_clamps_active() {
    let info = help(concat!(
        "function P(a : int) {}\n",
        "function Test() {\n",
        "  P(1, 2, |3);\n",
        "}\n",
    ))
    .expect("signature help past last param");

    assert_eq!(info.parameters.len(), 1);
    assert_eq!(info.active_parameter, Some(0));
}

#[test]
fn unclosed_call_still_produces_signature_help() {
    let info = help(concat!(
        "function Find(name : string, range : float) {}\n",
        "function Test() {\n",
        "  Find(a, |\n",
        "}\n",
    ))
    .expect("signature help for unclosed call");

    assert_eq!(info.label, "Find(name : string, range : float)");
    assert_eq!(info.active_parameter, Some(1));
}

#[test]
fn compact_colon_setting_drops_spaces_around_colon() {
    let info = help_with_colon(
        concat!(
            "function Find(name : string) : int {}\n",
            "function Test() {\n",
            "  Find(|);\n",
            "}\n",
        ),
        true,
    )
    .expect("signature help with compact colon");

    assert_eq!(info.label, "Find(name: string): int");
    let (s0, e0) = info.parameters[0];
    assert_eq!(&info.label[s0 as usize..e0 as usize], "name: string");
}

#[test]
fn non_call_position_returns_none() {
    let result = help(concat!("function Test() {\n", "  var x : int|;\n", "}\n",));

    assert!(result.is_none());
}
