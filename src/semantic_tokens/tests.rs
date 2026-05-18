use super::collect_semantic_tokens;
use crate::document::parse_document;
use crate::resolve::{SymbolDb, WorkspaceIndex};

const TEST_URI: &str = "file:///semtok_test.ws";

fn tokens_for(source: &str) -> Vec<u32> {
    let empty = WorkspaceIndex::default();
    tokens_for_with_db(source, &SymbolDb::new(&empty, &empty))
}

fn tokens_for_with_db(source: &str, db: &SymbolDb) -> Vec<u32> {
    let document = parse_document(source).expect("parse");
    collect_semantic_tokens(TEST_URI, &document, db)
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
    let base_doc = parse_document("class CActor {}\n").expect("base parse");
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///base/CActor.ws", &base_doc);

    let source = "class SomeClass {\n  var actor : CActor;\n}\n";
    let document = parse_document(source).expect("parse");
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &base);
    let data = collect_semantic_tokens(TEST_URI, &document, &db);
    let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
    assert!(
        types.contains(&super::TT_CLASS),
        "CActor from base scripts must produce a class token, got types: {types:?}"
    );
}

fn classified_tokens(source: &str, db: &SymbolDb) -> Vec<(String, u32)> {
    let document = parse_document(source).expect("parse");
    let data = collect_semantic_tokens(TEST_URI, &document, db);
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    let mut line: u32 = 0;
    let mut start: u32 = 0;
    for chunk in data.chunks_exact(5) {
        let delta_line = chunk[0];
        let delta_start = chunk[1];
        let length = chunk[2];
        let tt = chunk[3];
        line += delta_line;
        start = if delta_line > 0 {
            delta_start
        } else {
            start + delta_start
        };
        let line_text = lines.get(line as usize).copied().unwrap_or("");
        let text: String = line_text
            .chars()
            .skip(start as usize)
            .take(length as usize)
            .collect();
        out.push((text, tt));
    }
    out
}

#[test]
fn declaration_sites_get_expected_token_types() {
    struct Case {
        name: &'static str,
        source: &'static str,
        ident: &'static str,
        expected: u32,
    }
    let cases = [
        Case {
            name: "class declaration name",
            source: "class Foo {}\n",
            ident: "Foo",
            expected: super::TT_CLASS,
        },
        Case {
            name: "struct declaration name",
            source: "struct Bar {}\n",
            ident: "Bar",
            expected: super::TT_CLASS,
        },
        Case {
            name: "enum declaration name",
            source: "enum E { A }\n",
            ident: "E",
            expected: super::TT_ENUM,
        },
        Case {
            name: "enum variant declaration",
            source: "enum E { A }\n",
            ident: "A",
            expected: super::TT_ENUM_MEMBER,
        },
        Case {
            name: "function declaration name",
            source: "function Run() {}\n",
            ident: "Run",
            expected: super::TT_FUNCTION,
        },
        Case {
            name: "parameter declaration",
            source: "function F(p : int) {}\n",
            ident: "p",
            expected: super::TT_PARAMETER,
        },
        Case {
            name: "field declaration",
            source: "class C { var x : int; }\n",
            ident: "x",
            expected: super::TT_PROPERTY,
        },
        Case {
            name: "local variable declaration",
            source: "function F() { var z : int; }\n",
            ident: "z",
            expected: super::TT_VARIABLE,
        },
    ];
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &empty);
    for c in cases {
        let toks = classified_tokens(c.source, &db);
        let got = toks.iter().find(|(t, _)| t == c.ident).map(|(_, tt)| *tt);
        assert_eq!(
            got,
            Some(c.expected),
            "case '{}': ident '{}' expected token type {}, all tokens: {:?}",
            c.name,
            c.ident,
            c.expected,
            toks
        );
    }
}

#[test]
fn references_classify_like_their_declarations() {
    struct Case {
        name: &'static str,
        source: &'static str,
        ident: &'static str,
        expected_kind: u32,
        min_count: usize,
    }
    let cases = [
        Case {
            name: "local variable used after declaration",
            source: "function F() { var x : int; x = 1; }\n",
            ident: "x",
            expected_kind: super::TT_VARIABLE,
            min_count: 2,
        },
        Case {
            name: "parameter used inside body",
            source: "function F(p : int) { p = 1; }\n",
            ident: "p",
            expected_kind: super::TT_PARAMETER,
            min_count: 2,
        },
        Case {
            name: "top-level function call from another function",
            source: "function Foo() {}\nfunction Bar() { Foo(); }\n",
            ident: "Foo",
            expected_kind: super::TT_FUNCTION,
            min_count: 2,
        },
        Case {
            name: "repeated local reference within one body",
            source: "function F() { var x : int; x = 1; x = 2; x = 3; }\n",
            ident: "x",
            expected_kind: super::TT_VARIABLE,
            min_count: 4,
        },
    ];
    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&empty, &empty);
    for c in cases {
        let toks = classified_tokens(c.source, &db);
        let count = toks
            .iter()
            .filter(|(t, tt)| t == c.ident && *tt == c.expected_kind)
            .count();
        assert!(
            count >= c.min_count,
            "case '{}': expected at least {} tokens of type {} for ident '{}', got {} (all: {:?})",
            c.name,
            c.min_count,
            c.expected_kind,
            c.ident,
            count,
            toks
        );
    }
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
    let parent_src = "class Parent { var f : int; }\n";
    let parent_doc = parse_document(parent_src).expect("parent parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///parent.ws", &parent_doc);

    let child_src = "class Child extends Parent { function M() { f = 1; } }\n";
    let child_doc = parse_document(child_src).expect("child parse");
    workspace.update_document(TEST_URI, &child_doc);

    let empty = WorkspaceIndex::default();
    let db = SymbolDb::new(&workspace, &empty);
    let data = collect_semantic_tokens(TEST_URI, &child_doc, &db);

    let lines: Vec<&str> = child_src.lines().collect();
    let mut line: u32 = 0;
    let mut start: u32 = 0;
    let mut f_kind: Option<u32> = None;
    for chunk in data.chunks_exact(5) {
        line += chunk[0];
        start = if chunk[0] > 0 {
            chunk[1]
        } else {
            start + chunk[1]
        };
        let text: String = lines
            .get(line as usize)
            .copied()
            .unwrap_or("")
            .chars()
            .skip(start as usize)
            .take(chunk[2] as usize)
            .collect();
        if text == "f" {
            f_kind = Some(chunk[3]);
            break;
        }
    }
    assert_eq!(
        f_kind,
        Some(super::TT_PROPERTY),
        "inherited field 'f' should classify as property even when declared in a different file"
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
