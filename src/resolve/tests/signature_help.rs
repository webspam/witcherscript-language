use super::super::{SignatureHelpInfo, signature_help};
use crate::test_support::TestDb;

fn help(fixture: &str) -> Option<SignatureHelpInfo> {
    help_with_colon(fixture, false)
}

fn help_with_colon(fixture: &str, compact_colon: bool) -> Option<SignatureHelpInfo> {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    signature_help(&uri, t.doc_for(&uri), &t.db(), pos, compact_colon)
}

#[test]
fn reports_label_and_parameter_offsets() {
    let info = help(concat!(
        "function Find(name : string, range : float) : int {}\n",
        "function Test() {\n",
        "  Find($0);\n",
        "}\n",
    ))
    .expect("signature help in call");

    assert_eq!(info.label, "Find(name : string, range : float) : int");
    assert_eq!(info.parameters.len(), 2);
    assert_eq!(info.active_parameter, Some(0));

    let (s0, e0) = info.parameters[0];
    assert_eq!(&info.label[s0..e0], "name : string");
    let (s1, e1) = info.parameters[1];
    assert_eq!(&info.label[s1..e1], "range : float");
}

#[test]
fn active_parameter_advances_with_commas() {
    let source = concat!(
        "function Find(a : int, b : int, c : int) {}\n",
        "function Test() {\n",
        "  Find(1, $02, 3);\n",
        "}\n",
    );
    assert_eq!(help(source).unwrap().active_parameter, Some(1));

    let source = concat!(
        "function Find(a : int, b : int, c : int) {}\n",
        "function Test() {\n",
        "  Find(1, 2, $03);\n",
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
        "  p.GetHealth($0);\n",
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
        "    this.Helper($0);\n",
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
        "  NoArgs($0);\n",
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
        "  F(1, $02);\n",
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
        "  M(1, $02);\n",
        "}\n",
    ))
    .expect("signature help with multi-name param group");

    assert_eq!(info.label, "M(a : int, b : int)");
    assert_eq!(info.parameters.len(), 2);
    assert_eq!(info.active_parameter, Some(1));
}

#[test]
fn alias_param_and_return_types_are_normalised() {
    let info = help(concat!(
        "function F(x : Float, y : Int32) : Int32 {}\n",
        "function Test() {\n",
        "  F($0);\n",
        "}\n",
    ))
    .expect("signature help with engine-alias types");

    assert_eq!(info.label, "F(x : float, y : int) : int");
}

#[test]
fn generic_param_type_drops_source_spacing() {
    let info = help(concat!(
        "function F(xs : array< Foo >) {}\n",
        "function Test() {\n",
        "  F($0);\n",
        "}\n",
    ))
    .expect("signature help with spaced generic param");

    assert_eq!(info.label, "F(xs : array<Foo>)");
}

#[test]
fn array_method_substitutes_placeholder_in_signature() {
    let t = TestDb::new(concat!(
        "function Test() {\n",
        "  var xs : array<int>;\n",
        "  xs.PushBack($0);\n",
        "}\n",
    ))
    .with_builtins_index();
    let (uri, pos) = t.cursor();
    let info = signature_help(&uri, t.doc_for(&uri), &t.db(), pos, false)
        .expect("signature help on array method");

    assert_eq!(info.label, "PushBack(value : int)");
}

#[test]
fn nested_call_resolves_inner_callee() {
    let info = help(concat!(
        "function Inner(x : int) : int {}\n",
        "function Outer(y : int) : int {}\n",
        "function Test() {\n",
        "  Outer(Inner($0));\n",
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
        "  P(1, 2, $03);\n",
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
        "  Find(a, $0\n",
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
            "  Find($0);\n",
            "}\n",
        ),
        true,
    )
    .expect("signature help with compact colon");

    assert_eq!(info.label, "Find(name: string): int");
    let (s0, e0) = info.parameters[0];
    assert_eq!(&info.label[s0..e0], "name: string");
}

#[test]
fn non_call_position_returns_none() {
    let result = help(concat!("function Test() {\n", "  var x : int$0;\n", "}\n",));

    assert!(result.is_none());
}
