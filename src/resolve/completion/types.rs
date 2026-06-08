use std::sync::Arc;

use tree_sitter::Node;

use crate::cst::ancestors::{find_ancestor_of_kind, has_ancestor_of_kind};
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{AccessLevel, SymbolKind};

use super::super::Definition;
use super::super::ast::{
    is_kind_or_error_wrapped_kind, is_statement_boundary, is_type_annotation_boundary,
    nodes_at_offset, significant_node_before_byte,
};
use super::super::symbol_db::SymbolDb;

pub fn type_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    type_completions_arc(document, db, position)
        .map(|types| types.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn type_completions_arc(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Arc<[Definition]>> {
    type_completions_inner(document, db, position)
}

fn type_completions_inner(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Arc<[Definition]>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);
    let source = document.source.as_bytes();

    let in_type_context =
        // Gate 1: cursor immediately after a type-annotation colon
        significant_node_before_byte(root, source, byte_offset).is_some_and(is_type_annotation_boundary)
        // Gate 2: cursor on/within an ident whose start follows a type-annotation colon
        || nodes
            .last()
            .filter(|&n| is_kind_or_error_wrapped_kind(*n, &["ident"]))
            .and_then(|n| significant_node_before_byte(root, source, n.start_byte()))
            .is_some_and(is_type_annotation_boundary)
        // Gate 3: cursor already inside a type_annot subtree (generic type args, clean parses)
        || nodes.iter().any(|n| has_ancestor_of_kind(*n, &["type_annot"]));

    if !in_type_context {
        return None;
    }

    Some(db.merged_types_catalog())
}

pub fn annotation_name_completions(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<SourcePosition> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);

    let node = nodes.iter().find(|n| n.kind() == "annotation_ident")?;
    let prev = significant_node_before_byte(root, document.source.as_bytes(), node.start_byte());
    if prev.is_some_and(|p| !is_statement_boundary(p)) {
        return None;
    }
    Some(
        document
            .line_index
            .byte_to_position(&document.source, node.start_byte()),
    )
}

pub fn annotation_arg_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    annotation_arg_completions_inner(document, db, position).unwrap_or_default()
}

fn annotation_arg_completions_inner(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<Definition>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let in_annotation_arg = nodes_at_offset(root, byte_offset)
        .into_iter()
        .any(|n| has_annotation_arg_ancestor(n, byte_offset, &document.source));

    if !in_annotation_arg {
        return None;
    }

    Some(
        db.merged_types_catalog()
            .iter()
            .filter(|def| def.symbol.kind == SymbolKind::Class)
            .cloned()
            .collect(),
    )
}

const CLASS_ARG_ANNOTATIONS: &[&str] =
    &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"];

fn has_annotation_arg_ancestor(node: Node, byte_offset: usize, source: &str) -> bool {
    // Empty parens (`@wrapMethod()`) fail the `'(' ident ')'` rule and recover as an
    // ERROR node holding the same annotation_ident/`(`/`)` children, so accept both.
    find_ancestor_of_kind(node, &["annotation", "ERROR"]).is_some_and(|container| {
        takes_class_arg(container, source) && is_inside_annotation_parens(container, byte_offset)
    })
}

fn takes_class_arg(annotation: Node, source: &str) -> bool {
    annotation
        .children(&mut annotation.walk())
        .find(|c| c.kind() == "annotation_ident")
        .map(|n| &source[n.start_byte()..n.end_byte()])
        .is_some_and(|name| CLASS_ARG_ANNOTATIONS.contains(&name))
}

fn is_inside_annotation_parens(annotation: Node, byte_offset: usize) -> bool {
    let mut cursor = annotation.walk();
    let mut saw_open = false;
    for child in annotation.children(&mut cursor) {
        match child.kind() {
            "(" => saw_open = true,
            ")" => {
                if byte_offset <= child.start_byte() {
                    return saw_open;
                }
                return false;
            }
            _ => {}
        }
    }
    saw_open
}

/// Which method body the override inserts. `@wrapMethod` calls `wrappedMethod`; `@replaceMethod` does not.
#[derive(Debug, Clone, Copy)]
pub enum OverrideBody {
    Wrap,
    Replace,
}

#[derive(Debug)]
pub struct OverrideCompletion {
    pub methods: Vec<Definition>,
    /// `function` keyword not yet typed; each insert must lead with it.
    pub needs_function_keyword: bool,
    pub body: OverrideBody,
}

pub fn override_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<OverrideCompletion> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let root = document.tree.root_node();
    let source = document.source.as_bytes();

    // A `function` keyword already typed means the insert must not repeat it.
    let prev = nodes_at_offset(root, byte_offset)
        .last()
        .filter(|n| matches!(n.kind(), "ident" | "function"))
        .and_then(|n| significant_node_before_byte(root, source, n.start_byte()))
        .or_else(|| significant_node_before_byte(root, source, byte_offset))?;
    let (anchor, needs_function_keyword) = if prev.kind() == "function" {
        (
            significant_node_before_byte(root, source, prev.start_byte())?,
            false,
        )
    } else {
        (prev, true)
    };

    let (methods, body) = match override_target(anchor, byte_offset, &document.source)? {
        OverrideTarget::ClassMethods { class, body } => (direct_methods_of_class(class, db)?, body),
        OverrideTarget::GlobalFunctions => (db.all_top_level_callables(), OverrideBody::Replace),
    };
    Some(OverrideCompletion {
        methods,
        needs_function_keyword,
        body,
    })
}

enum OverrideTarget<'a> {
    ClassMethods { class: &'a str, body: OverrideBody },
    GlobalFunctions,
}

fn override_target<'a>(
    anchor: Node,
    byte_offset: usize,
    source: &'a str,
) -> Option<OverrideTarget<'a>> {
    if anchor.kind() == ")" {
        let (class, body) = class_override_target(anchor, source)?;
        return Some(OverrideTarget::ClassMethods { class, body });
    }
    // `@replaceMethod` without `()` replaces a global; cursor past it (whitespace) rules out still typing the annotation.
    if is_replace_method_ident(anchor, source) && byte_offset > anchor.end_byte() {
        return Some(OverrideTarget::GlobalFunctions);
    }
    None
}

fn is_replace_method_ident(node: Node, source: &str) -> bool {
    node.kind() == "annotation_ident"
        && &source[node.start_byte()..node.end_byte()] == "@replaceMethod"
}

fn direct_methods_of_class(class_name: &str, db: &SymbolDb) -> Option<Vec<Definition>> {
    let class_def = db.find_top_level(class_name)?;
    if class_def.symbol.kind != SymbolKind::Class {
        return None;
    }
    Some(
        db.direct_members_of(class_name, AccessLevel::Private)
            .into_iter()
            .filter(|def| matches!(def.symbol.kind, SymbolKind::Method | SymbolKind::Event))
            .collect(),
    )
}

/// A `)` closing `@wrapMethod(Class)` / `@replaceMethod(Class)` yields the class name and body shape.
fn class_override_target<'a>(anchor: Node, source: &'a str) -> Option<(&'a str, OverrideBody)> {
    if anchor.kind() != ")" {
        return None;
    }
    let annotation = anchor.parent()?;
    if annotation.kind() != "annotation" {
        return None;
    }
    let name = annotation
        .children(&mut annotation.walk())
        .find(|c| c.kind() == "annotation_ident")
        .map(|n| &source[n.start_byte()..n.end_byte()])?;
    let body = match name {
        "@wrapMethod" => OverrideBody::Wrap,
        "@replaceMethod" => OverrideBody::Replace,
        _ => return None,
    };
    let class = annotation
        .children(&mut annotation.walk())
        .find(|c| c.kind() == "ident")
        .map(|n| &source[n.start_byte()..n.end_byte()])?;
    Some((class, body))
}

pub fn extends_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    let Some(header) = super::headers::header_state_and_kind(document, position) else {
        return Vec::new();
    };
    if header.state != super::headers::HeaderState::AfterExtendsKw {
        return Vec::new();
    }
    let self_name = header.self_name.as_deref();
    match header.kind {
        Some(super::headers::HeaderDeclKind::Class) => db
            .all_types()
            .into_iter()
            .filter(|def| def.symbol.kind == SymbolKind::Class)
            .filter(|def| Some(def.symbol.name.as_str()) != self_name)
            .collect(),
        Some(super::headers::HeaderDeclKind::State) => {
            let Some(owner) = header.owner_name.as_deref() else {
                return Vec::new();
            };
            let chain = super::headers::class_chain(db, owner);
            if chain.is_empty() {
                return Vec::new();
            }
            db.all_types()
                .into_iter()
                .filter(|def| def.symbol.kind == SymbolKind::State)
                .filter(|def| {
                    def.symbol
                        .owner_class
                        .as_deref()
                        .is_some_and(|o| chain.iter().any(|c| c == o))
                })
                .filter(|def| Some(def.symbol.name.as_str()) != self_name)
                .collect()
        }
        None => Vec::new(),
    }
}

pub fn state_owner_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    let Some(header) = super::headers::header_state_and_kind(document, position) else {
        return Vec::new();
    };
    if header.state != super::headers::HeaderState::AfterInKw {
        return Vec::new();
    }
    db.all_types()
        .into_iter()
        .filter(|def| def.symbol.kind == SymbolKind::Class)
        .collect()
}
