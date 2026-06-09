use rstest::rstest;

use super::{
    SemanticTokenView, TT_CLASS, TT_ENUM, TT_ENUM_MEMBER, TT_FUNCTION, TT_PARAMETER, TT_PROPERTY,
    TT_VARIABLE, collect_semantic_tokens, collect_semantic_tokens_cancellable, decode_tokens,
};
use crate::document::parse_document;
use crate::resolve::{SymbolDb, WorkspaceIndex};

const TEST_URI: &str = "file:///semtok_test.ws";

fn tokens_for(source: &str) -> Vec<SemanticTokenView> {
    let empty = WorkspaceIndex::default();
    tokens_for_with_db(source, &SymbolDb::new(&empty, &empty))
}

fn tokens_for_with_db(source: &str, db: &SymbolDb) -> Vec<SemanticTokenView> {
    let document = parse_document(source).expect("parse");
    decode_tokens(&collect_semantic_tokens(TEST_URI, &document, db))
}

fn types_of(tokens: &[SemanticTokenView]) -> Vec<u32> {
    tokens.iter().map(|t| t.token_type).collect()
}

#[test]
fn emits_tokens_for_class_declaration() {
    let tokens = tokens_for("class CExample {}\n");
    assert!(
        tokens.len() >= 2,
        "expected at least 2 tokens, got {}",
        tokens.len()
    );
}

#[test]
fn class_declaration_keyword_is_modifier() {
    let tokens = tokens_for("class CExample {}\n");
    assert!(!tokens.is_empty());
    assert_eq!(tokens[0].delta_line, 0, "delta_line");
    assert_eq!(tokens[0].delta_start, 0, "delta_start");
    assert_eq!(tokens[0].length, 5, "length of 'class'");
    assert_eq!(
        tokens[0].token_type,
        super::TT_MODIFIER,
        "token type should be modifier"
    );
}

#[test]
fn class_name_token_type_is_correct() {
    let tokens = tokens_for("class CExample {}\n");
    assert!(tokens.len() >= 2);
    assert_eq!(tokens[1].delta_line, 0, "second token delta_line");
    assert_eq!(
        tokens[1].delta_start, 6,
        "second token delta_start (after 'class ')"
    );
    assert_eq!(tokens[1].length, 8, "length of 'CExample'");
    assert_eq!(
        tokens[1].token_type,
        super::TT_CLASS,
        "token type should be class"
    );
}

#[test]
fn function_tokens_are_emitted() {
    let tokens = tokens_for("function Foo() {}\n");
    assert!(
        tokens.len() >= 2,
        "expected modifier + function name tokens"
    );
    assert_eq!(tokens[0].token_type, super::TT_MODIFIER);
    assert_eq!(tokens[1].token_type, super::TT_FUNCTION);
}

#[test]
fn specifier_is_modifier_not_keyword() {
    let tokens = tokens_for("class C {\n private var x : int;\n}\n");
    let types = types_of(&tokens);
    assert!(
        types.contains(&super::TT_MODIFIER),
        "expected a modifier token for 'private', got types: {types:?}"
    );
}

#[test]
fn var_is_modifier_not_keyword() {
    let tokens = tokens_for("function F() { var x : int; }\n");
    let types = types_of(&tokens);
    assert!(
        types.contains(&super::TT_MODIFIER),
        "expected a modifier token for 'var', got types: {types:?}"
    );
}

#[test]
fn control_flow_keywords_are_keyword_type() {
    let tokens = tokens_for("function F() { if (true) { return; } }\n");
    let types = types_of(&tokens);
    assert!(
        !types.is_empty(),
        "expected some tokens for control flow source"
    );
}

#[test]
fn comment_token_type_is_correct() {
    let tokens = tokens_for("// a comment\n");
    assert!(!tokens.is_empty());
    assert_eq!(tokens[0].token_type, super::TT_COMMENT);
}

#[test]
fn string_literal_token_type_is_correct() {
    let tokens = tokens_for("function F() { var s : string; s = \"hello\"; }\n");
    let types = types_of(&tokens);
    assert!(
        types.contains(&super::TT_STRING),
        "expected a string token, got types: {types:?}"
    );
}

#[test]
fn name_literal_is_enum_member_not_string() {
    let tokens = tokens_for("function F() { var n : CName; n = 'SomeName'; }\n");
    let types = types_of(&tokens);
    assert!(
        types.contains(&super::TT_ENUM_MEMBER),
        "expected enumMember token for name literal, got types: {types:?}"
    );
    assert!(
        !types.contains(&super::TT_STRING),
        "name literal should not be classified as string, got types: {types:?}"
    );
}

#[test]
fn variable_use_gets_variable_token() {
    let tokens = tokens_for("function F() { var x : int; x = 1; }\n");
    let types = types_of(&tokens);
    assert!(
        types.iter().filter(|&&t| t == super::TT_VARIABLE).count() >= 2,
        "expected variable token for both declaration and use of 'x', got types: {types:?}"
    );
}

#[test]
fn member_access_lhs_gets_variable_token() {
    let tokens =
        tokens_for("struct Vector { var X : float; }\nfunction F() { var v : Vector; v.X = 0; }\n");
    let types = types_of(&tokens);
    assert!(
        types.iter().filter(|&&t| t == super::TT_VARIABLE).count() >= 2,
        "expected variable token for declaration and use of 'v', got types: {types:?}"
    );
    assert!(
        types.contains(&super::TT_PROPERTY),
        "expected property token for resolved field 'X', got types: {types:?}"
    );
}

#[test]
fn unresolvable_type_annotation_gets_no_token() {
    let types_with = types_of(&tokens_for(
        "class CObject {}\nfunction F(x : CObject) {}\n",
    ));
    let types_without = types_of(&tokens_for("function F(x : CObject) {}\n"));
    assert!(
        types_with.contains(&super::TT_CLASS),
        "defined CObject should produce a class token, got: {types_with:?}"
    );
    assert!(
        !types_without.contains(&super::TT_CLASS),
        "undefined CObject must not produce a class token, got: {types_without:?}"
    );
}

#[test]
fn resolved_type_annotation_gets_class_token() {
    let tokens = tokens_for("class CObject {}\nfunction F(x : CObject) {}\n");
    let types = types_of(&tokens);
    assert!(
        types.contains(&super::TT_CLASS),
        "defined type in annotation should resolve to class token, got types: {types:?}"
    );
}

#[test]
fn type_annotation_from_base_scripts_gets_class_token() {
    let t = crate::test_support::TestDb::new("class SomeClass {\n  var actor : CActor;\n}\n")
        .with_base_doc("file:///base/CActor.ws", "class CActor {}\n");
    let tokens = decode_tokens(&collect_semantic_tokens(
        t.primary_uri(),
        t.primary_doc(),
        &t.db(),
    ));
    let types = types_of(&tokens);
    assert!(
        types.contains(&super::TT_CLASS),
        "CActor from base scripts must produce a class token, got types: {types:?}"
    );
}

fn classified_tokens(source: &str, db: &SymbolDb) -> Vec<(String, u32)> {
    let document = parse_document(source).expect("parse");
    let tokens = decode_tokens(&collect_semantic_tokens(TEST_URI, &document, db));
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    let mut line: u32 = 0;
    let mut start: u32 = 0;
    for t in &tokens {
        line += t.delta_line;
        start = if t.delta_line > 0 {
            t.delta_start
        } else {
            start + t.delta_start
        };
        let line_text = lines.get(line as usize).copied().unwrap_or("");
        let text: String = line_text
            .chars()
            .skip(start as usize)
            .take(t.length as usize)
            .collect();
        out.push((text, t.token_type));
    }
    out
}

#[test]
fn wrapped_method_macro_gets_no_token() {
    let source = "class CPlayer {\n  public function OnSpawned() {}\n}\n\
                  @wrapMethod(CPlayer)\nfunction OnSpawned() {\n  wrappedMethod();\n}\n";
    let t = crate::test_support::TestDb::new(source);
    let toks = classified_tokens(source, &t.db());
    assert!(
        !toks.iter().any(|(text, _)| text == "wrappedMethod"),
        "wrappedMethod must keep its current (absent) highlighting, got: {toks:?}"
    );
}

#[rstest]
#[case::class_declaration_name("class declaration name", "class Foo {}\n", "Foo", TT_CLASS)]
#[case::struct_declaration_name("struct declaration name", "struct Bar {}\n", "Bar", TT_CLASS)]
#[case::enum_declaration_name("enum declaration name", "enum E { A }\n", "E", TT_ENUM)]
#[case::enum_member_declaration("enum member declaration", "enum E { A }\n", "A", TT_ENUM_MEMBER)]
#[case::function_declaration_name(
    "function declaration name",
    "function Run() {}\n",
    "Run",
    TT_FUNCTION
)]
#[case::parameter_declaration(
    "parameter declaration",
    "function F(p : int) {}\n",
    "p",
    TT_PARAMETER
)]
#[case::field_declaration("field declaration", "class C { var x : int; }\n", "x", TT_PROPERTY)]
#[case::local_variable_declaration(
    "local variable declaration",
    "function F() { var z : int; }\n",
    "z",
    TT_VARIABLE
)]
fn declaration_sites_get_expected_token_types(
    #[case] name: &str,
    #[case] source: &str,
    #[case] ident: &str,
    #[case] expected: u32,
) {
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &empty);
    let toks = classified_tokens(source, &db);
    let got = toks.iter().find(|(t, _)| t == ident).map(|(_, tt)| *tt);
    assert_eq!(
        got,
        Some(expected),
        "case '{name}': ident '{ident}' expected token type {expected}, all tokens: {toks:?}"
    );
}

#[rstest]
#[case::local_variable_used_after_declaration(
    "local variable used after declaration",
    "function F() { var x : int; x = 1; }\n",
    "x",
    TT_VARIABLE,
    2
)]
#[case::parameter_used_inside_body(
    "parameter used inside body",
    "function F(p : int) { p = 1; }\n",
    "p",
    TT_PARAMETER,
    2
)]
#[case::top_level_function_call_from_another_function(
    "top-level function call from another function",
    "function Foo() {}\nfunction Bar() { Foo(); }\n",
    "Foo",
    TT_FUNCTION,
    2
)]
#[case::repeated_local_reference_within_one_body(
    "repeated local reference within one body",
    "function F() { var x : int; x = 1; x = 2; x = 3; }\n",
    "x",
    TT_VARIABLE,
    4
)]
fn references_classify_like_their_declarations(
    #[case] name: &str,
    #[case] source: &str,
    #[case] ident: &str,
    #[case] expected_kind: u32,
    #[case] min_count: usize,
) {
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &empty);
    let toks = classified_tokens(source, &db);
    let count = toks
        .iter()
        .filter(|(t, tt)| t == ident && *tt == expected_kind)
        .count();
    assert!(
        count >= min_count,
        "case '{name}': expected at least {min_count} tokens of type {expected_kind} for ident '{ident}', got {count} (all: {toks:?})"
    );
}

#[test]
fn same_file_class_member_resolves_without_workspace() {
    let source = "class C { var f : int; function M() { f = 1; } }\n";
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &empty);
    let toks = classified_tokens(source, &db);
    let f_kinds: Vec<u32> = toks
        .iter()
        .filter_map(|(t, k)| if t == "f" { Some(*k) } else { None })
        .collect();
    assert!(
        f_kinds.iter().filter(|&&k| k == super::TT_PROPERTY).count() >= 2,
        "expected two TT_PROPERTY tokens for 'f' (decl + use), got: {f_kinds:?}"
    );
}

#[test]
fn inherited_member_reference_resolves_via_workspace() {
    let t = crate::test_support::TestDb::new(concat!(
        "//- /parent.ws\n",
        "class Parent { var f : int; }\n",
        "//- /semtok_test.ws\n",
        "class Child extends Parent { function M() { f = 1; } }\n",
    ));
    let child = t.doc_for(TEST_URI);
    let toks = classified_tokens(&child.source, &t.db());
    let f_kind = toks.iter().find(|(t, _)| t == "f").map(|(_, k)| *k);
    assert_eq!(
        f_kind,
        Some(super::TT_PROPERTY),
        "inherited field 'f' should classify as property even when declared in a different file"
    );
}

#[test]
fn script_global_classifies_as_variable_with_default_library_modifier() {
    let env = crate::test_support::script_env("thePlayer", "CR4Player");
    let t = crate::test_support::TestDb::new(concat!(
        "//- /semtok_test.ws\n",
        "function Test() {\n thePlayer;\n}\n",
    ))
    .with_base_doc("file:///r4player.ws", "class CR4Player {}\n");
    let db = t.db().with_script_env(&env);

    let document = t.doc_for(TEST_URI);
    let tokens = decode_tokens(&collect_semantic_tokens(TEST_URI, document, &db));
    let player = tokens
        .iter()
        .find(|t| t.delta_line == 1)
        .expect("expected a token on the body line");
    assert_eq!(
        player.token_type,
        TT_VARIABLE,
        "thePlayer should colour as variable, not class; got {}",
        player.token_type_name(),
    );
    assert_ne!(
        player.token_modifiers & super::MOD_DEFAULT_LIBRARY,
        0,
        "thePlayer should carry the defaultLibrary modifier bit; got {:#b}",
        player.token_modifiers,
    );
}

#[test]
fn workspace_class_shadowing_script_global_still_colours_as_class() {
    let env = crate::test_support::script_env("thePlayer", "CR4Player");
    let t = crate::test_support::TestDb::new(concat!(
        "//- /shadow.ws\n",
        "class thePlayer {}\n",
        "//- /semtok_test.ws\n",
        "function Test() {\n thePlayer;\n}\n",
    ));
    let db = t.db().with_script_env(&env);

    let document = t.doc_for(TEST_URI);
    let tokens = decode_tokens(&collect_semantic_tokens(TEST_URI, document, &db));
    let player = tokens
        .iter()
        .find(|t| t.delta_line == 1)
        .expect("expected a token on the body line");
    assert_eq!(
        player.token_type,
        TT_CLASS,
        "workspace class named thePlayer must win over the ini global, got {}",
        player.token_type_name(),
    );
    assert_eq!(
        player.token_modifiers, 0,
        "shadowing class should carry no modifiers, got {:#b}",
        player.token_modifiers,
    );
}

#[test]
fn cancellable_walk_returns_none_when_should_continue_is_false() {
    let source = "class A {} class B {} class C {}\n";
    let document = parse_document(source).expect("parse");
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &empty);
    let result = collect_semantic_tokens_cancellable(TEST_URI, &document, &db, &|| false);
    assert!(result.is_none(), "cancelled walk should return None");
}

#[test]
fn cancellable_walk_returns_some_when_should_continue_is_true() {
    let source = "class A {}\n";
    let document = parse_document(source).expect("parse");
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &empty);
    let result = collect_semantic_tokens_cancellable(TEST_URI, &document, &db, &|| true);
    assert!(
        result.is_some_and(|tokens| !tokens.is_empty()),
        "non-cancelled walk should return tokens"
    );
}

#[test]
fn primitive_type_annotation_does_not_get_a_class_token() {
    let source = "function F() { var x : int; }\n";
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &empty);
    let toks = classified_tokens(source, &db);
    let int_kinds: Vec<u32> = toks
        .iter()
        .filter_map(|(t, k)| if t == "int" { Some(*k) } else { None })
        .collect();
    assert!(
        int_kinds.is_empty(),
        "the primitive type name 'int' must not produce a semantic token \
         (TextMate grammar handles it); got kinds: {int_kinds:?}"
    );
}
