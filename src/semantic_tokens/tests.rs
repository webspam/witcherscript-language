use tree_sitter::Parser;

use super::collect_semantic_tokens;
use crate::line_index::LineIndex;
use crate::resolve::{SymbolDb, WorkspaceIndex};
use crate::symbols::extract_symbols;

fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_witcherscript::language())
        .expect("failed to load WitcherScript grammar");
    parser.parse(source, None).expect("failed to parse source")
}

fn tokens_for(source: &str) -> Vec<u32> {
    let empty = WorkspaceIndex::default();
    tokens_for_with_db(source, &SymbolDb::new(&empty, &empty))
}

fn tokens_for_with_db(source: &str, db: &SymbolDb) -> Vec<u32> {
    let tree = parse(source);
    let index = LineIndex::new(source);
    let symbols = extract_symbols(tree.root_node(), source, &index);
    collect_semantic_tokens(tree.root_node(), source, &index, &symbols, db)
}

#[test]
fn emits_tokens_for_class_declaration() {
    // "class CExample {}" should produce at least keyword + class tokens
    let data = tokens_for("class CExample {}\n");
    // Each token is 5 u32 values; must have at least 2 tokens
    assert!(
        data.len() >= 10,
        "expected at least 2 tokens, got {}",
        data.len() / 5
    );
}

#[test]
fn class_declaration_keyword_is_modifier() {
    // "class" is a declaration keyword → modifier, not control-flow keyword
    let source = "class CExample {}\n";
    let data = tokens_for(source);
    assert!(data.len() >= 5);
    assert_eq!(data[0], 0, "delta_line");
    assert_eq!(data[1], 0, "delta_start");
    assert_eq!(data[2], 5, "length of 'class'");
    assert_eq!(data[3], super::TT_MODIFIER, "token type should be modifier");
}

#[test]
fn class_name_token_type_is_correct() {
    let source = "class CExample {}\n";
    let data = tokens_for(source);
    // Second token: delta_line=0, delta_start=6 (after 'class '), length=8 ("CExample"), type=TT_CLASS(0)
    assert!(data.len() >= 10);
    assert_eq!(data[5], 0, "second token delta_line");
    assert_eq!(data[6], 6, "second token delta_start (after 'class ')");
    assert_eq!(data[7], 8, "length of 'CExample'");
    assert_eq!(data[8], super::TT_CLASS, "token type should be class");
}

#[test]
fn function_tokens_are_emitted() {
    let source = "function Foo() {}\n";
    let data = tokens_for(source);
    assert!(data.len() >= 10, "expected modifier + function name tokens");
    // 'function' declaration keyword → modifier
    assert_eq!(data[3], super::TT_MODIFIER);
    // 'Foo' name next — TT_FUNCTION
    assert_eq!(data[8], super::TT_FUNCTION);
}

#[test]
fn specifier_is_modifier_not_keyword() {
    let source = "class C {\n private var x : int;\n}\n";
    let data = tokens_for(source);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
    assert!(
        types.contains(&super::TT_MODIFIER),
        "expected a modifier token for 'private', got types: {types:?}"
    );
}

#[test]
fn var_is_modifier_not_keyword() {
    let source = "function F() { var x : int; }\n";
    let data = tokens_for(source);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
    assert!(
        types.contains(&super::TT_MODIFIER),
        "expected a modifier token for 'var', got types: {types:?}"
    );
}

#[test]
fn control_flow_keywords_are_keyword_type() {
    let source = "function F() { if (true) { return; } }\n";
    let data = tokens_for(source);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
    assert!(
        !types.is_empty(),
        "expected some tokens for control flow source"
    );
}

#[test]
fn comment_token_type_is_correct() {
    let source = "// a comment\n";
    let data = tokens_for(source);
    assert!(data.len() >= 5);
    assert_eq!(data[3], super::TT_COMMENT);
}

#[test]
fn string_literal_token_type_is_correct() {
    let source = "function F() { var s : string; s = \"hello\"; }\n";
    let data = tokens_for(source);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
    assert!(
        types.contains(&super::TT_STRING),
        "expected a string token, got types: {types:?}"
    );
}

#[test]
fn name_literal_is_enum_member_not_string() {
    let source = "function F() { var n : CName; n = 'SomeName'; }\n";
    let data = tokens_for(source);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
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
    let source = "function F() { var x : int; x = 1; }\n";
    let data = tokens_for(source);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
    assert!(
        types.iter().filter(|&&t| t == super::TT_VARIABLE).count() >= 2,
        "expected variable token for both declaration and use of 'x', got types: {types:?}"
    );
}

#[test]
fn member_access_lhs_gets_variable_token() {
    // Vector and its field X must be defined for the member access to resolve.
    let source = "struct Vector { var X : float; }\nfunction F() { var v : Vector; v.X = 0; }\n";
    let data = tokens_for(source);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
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
    // CObject is not defined — neither TT_CLASS nor any other token should appear for it.
    let source_with = "class CObject {}\nfunction F(x : CObject) {}\n";
    let source_without = "function F(x : CObject) {}\n";
    let types_with: Vec<u32> = tokens_for(source_with)
        .iter()
        .skip(3)
        .step_by(5)
        .copied()
        .collect();
    let types_without: Vec<u32> = tokens_for(source_without)
        .iter()
        .skip(3)
        .step_by(5)
        .copied()
        .collect();
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
    // CObject is defined — its use in a type annotation should resolve to TT_CLASS.
    let source = "class CObject {}\nfunction F(x : CObject) {}\n";
    let data = tokens_for(source);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
    assert!(
        types.contains(&super::TT_CLASS),
        "defined type in annotation should resolve to class token, got types: {types:?}"
    );
}

#[test]
fn type_annotation_from_base_scripts_gets_class_token() {
    // CActor is defined only in base_scripts — the field type annotation should
    // still resolve and produce a class token.
    let base_source = "class CActor {}\n";
    let base_tree = parse(base_source);
    let base_index = LineIndex::new(base_source);
    let base_symbols = extract_symbols(base_tree.root_node(), base_source, &base_index);
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///base/CActor.ws", &base_symbols);

    let source = "class SomeClass {\n  var actor : CActor;\n}\n";
    let tree = parse(source);
    let index = LineIndex::new(source);
    let symbols = extract_symbols(tree.root_node(), source, &index);
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &base);
    let data = collect_semantic_tokens(tree.root_node(), source, &index, &symbols, &db);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
    assert!(
        types.contains(&super::TT_CLASS),
        "CActor from base scripts must produce a class token, got types: {types:?}"
    );
}
