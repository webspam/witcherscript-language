use tree_sitter::Node;

use super::{NameContext, classify_ident_context};
use crate::cst::kinds;
use crate::document::parse_document;
use crate::symbols::SymbolKind;

fn find_ident_at_offset(root: Node<'_>, byte_offset: usize) -> Option<Node<'_>> {
    let node = root.descendant_for_byte_range(byte_offset, byte_offset)?;
    if node.kind() == kinds::IDENT {
        return Some(node);
    }
    node.parent().filter(|p| p.kind() == kinds::IDENT)
}

fn classify_at(fixture: &str) -> Option<NameContext> {
    let cursor = fixture.find('$').expect("fixture must include $ marker");
    let cleaned = fixture.replacen('$', "", 1);
    let doc = parse_document(cleaned.clone()).expect("parse");
    let ident = find_ident_at_offset(doc.tree.root_node(), cursor).expect("ident at cursor");
    classify_ident_context(ident, cleaned.as_bytes())
}

#[test]
fn type_annotation_is_type() {
    let ctx = classify_at("function F() { var x : $Foo; }\n").unwrap();
    assert_eq!(ctx, NameContext::Type);
}

#[test]
fn new_expr_is_type() {
    let ctx = classify_at("function F() { var x : C; x = new $C in this; }\n").unwrap();
    assert_eq!(ctx, NameContext::Type);
}

#[test]
fn cast_is_type() {
    let ctx = classify_at("function F() { var c : C; var d : D; d = ($D) c; }\n").unwrap();
    assert_eq!(ctx, NameContext::Type);
}

#[test]
fn annotation_arg_is_type() {
    let ctx = classify_at("@addMethod($Foo) function R() {}\n").unwrap();
    assert_eq!(ctx, NameContext::Type);
}

#[test]
fn class_extends_is_type() {
    let ctx = classify_at("class A extends $B {}\n").unwrap();
    assert_eq!(ctx, NameContext::Type);
}

#[test]
fn state_owner_is_type() {
    let ctx = classify_at("state S in $Owner {}\n").unwrap();
    assert_eq!(ctx, NameContext::Type);
}

#[test]
fn state_extends_carries_owner() {
    let ctx = classify_at("state Child in Owner extends $BaseState {}\n").unwrap();
    assert_eq!(
        ctx,
        NameContext::StateExtends {
            owner_class: "Owner".to_string()
        }
    );
}

#[test]
fn bare_call_is_callable() {
    let ctx = classify_at("function F() { $Helper(); }\n").unwrap();
    assert_eq!(ctx, NameContext::Callable);
}

#[test]
fn bare_value_is_value() {
    let ctx = classify_at("function F() { var x : int; x = $y; }\n").unwrap();
    assert_eq!(ctx, NameContext::Value);
}

#[test]
fn declaration_returns_none() {
    let ctx = classify_at("function $F() {}\n");
    assert!(ctx.is_none(), "declaration should not be a name lookup");
}

#[test]
fn member_access_returns_none() {
    let ctx = classify_at("function F() { var a : A; a.$known = 1; }\n");
    assert!(ctx.is_none(), "member access is resolved separately");
}

#[test]
fn type_accepts_class_struct_enum_only() {
    let ctx = NameContext::Type;
    assert!(ctx.accepts(SymbolKind::Class));
    assert!(ctx.accepts(SymbolKind::Struct));
    assert!(ctx.accepts(SymbolKind::Enum));
    assert!(!ctx.accepts(SymbolKind::State));
    assert!(!ctx.accepts(SymbolKind::Function));
    assert!(!ctx.accepts(SymbolKind::Event));
}

#[test]
fn callable_accepts_function_event_struct() {
    let ctx = NameContext::Callable;
    assert!(ctx.accepts(SymbolKind::Function));
    assert!(ctx.accepts(SymbolKind::Event));
    assert!(ctx.accepts(SymbolKind::Struct));
    assert!(!ctx.accepts(SymbolKind::Class));
    assert!(!ctx.accepts(SymbolKind::State));
    assert!(!ctx.accepts(SymbolKind::Enum));
}

#[test]
fn value_excludes_state() {
    let ctx = NameContext::Value;
    assert!(ctx.accepts(SymbolKind::Function));
    assert!(ctx.accepts(SymbolKind::Event));
    assert!(ctx.accepts(SymbolKind::Class));
    assert!(ctx.accepts(SymbolKind::Struct));
    assert!(ctx.accepts(SymbolKind::Enum));
    assert!(!ctx.accepts(SymbolKind::State));
}

#[test]
fn state_extends_accepts_state_only() {
    let ctx = NameContext::StateExtends {
        owner_class: "Owner".to_string(),
    };
    assert!(ctx.accepts(SymbolKind::State));
    assert!(!ctx.accepts(SymbolKind::Class));
    assert!(!ctx.accepts(SymbolKind::Function));
}
