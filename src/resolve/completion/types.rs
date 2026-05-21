use tree_sitter::Node;

use crate::cst::ancestors::{find_ancestor_of_kind, has_ancestor_of_kind};
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{AccessLevel, SymbolKind};

use super::super::ast::{
    is_kind_or_error_wrapped_kind, is_statement_boundary, is_type_annotation_boundary,
    nodes_at_offset, significant_node_before_byte,
};
use super::super::db::SymbolDb;
use super::super::Definition;

pub fn type_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    type_completions_inner(document, db, position).unwrap_or_default()
}

fn type_completions_inner(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<Definition>> {
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

    Some(db.all_types())
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
        db.all_types()
            .into_iter()
            .filter(|def| def.symbol.kind == SymbolKind::Class)
            .collect(),
    )
}

const CLASS_ARG_ANNOTATIONS: &[&str] =
    &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"];

fn has_annotation_arg_ancestor(node: Node, byte_offset: usize, source: &str) -> bool {
    find_ancestor_of_kind(node, &["annotation"]).is_some_and(|annotation| {
        takes_class_arg(annotation, source) && is_inside_annotation_parens(annotation, byte_offset)
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

#[derive(Debug)]
pub enum AfterWrapMethodCompletions {
    /// Cursor is directly after `@wrapMethod(CClass)` — only `function` is valid next.
    FunctionKeyword,
    /// Cursor is after `@wrapMethod(CClass)\nfunction ` — offer methods of the class.
    MethodList(Vec<Definition>),
}

pub fn after_wrap_method_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<AfterWrapMethodCompletions> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let source = document.source.as_bytes();

    // If the cursor is ON an ident or `function` token, step back to the node
    // before that token's start; otherwise step back from the cursor directly.
    let effective_prev = nodes_at_offset(root, byte_offset)
        .last()
        .filter(|n| matches!(n.kind(), "ident" | "function"))
        .and_then(|n| significant_node_before_byte(root, source, n.start_byte()))
        .or_else(|| significant_node_before_byte(root, source, byte_offset))?;

    // Stage 2: `function` keyword is the boundary — cursor is after it or typing a name.
    if effective_prev.kind() == "function" {
        let before_fn = significant_node_before_byte(root, source, effective_prev.start_byte())?;
        let class_name = wrap_method_class_from_closing_paren(before_fn, &document.source)?;
        return Some(AfterWrapMethodCompletions::MethodList(
            direct_methods_of_class(class_name, db)?,
        ));
    }

    // Stage 1: `)` of annotation is the boundary — `function` keyword not yet complete.
    let class_name = wrap_method_class_from_closing_paren(effective_prev, &document.source)?;
    let class_def = db.find_top_level(class_name)?;
    if class_def.symbol.kind != SymbolKind::Class {
        return None;
    }
    Some(AfterWrapMethodCompletions::FunctionKeyword)
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

fn wrap_method_class_from_closing_paren<'a>(node: Node, source: &'a str) -> Option<&'a str> {
    if node.kind() != ")" {
        return None;
    }
    let annotation = node.parent()?;
    if annotation.kind() != "annotation" {
        return None;
    }
    let annotation_name = annotation
        .children(&mut annotation.walk())
        .find(|c| c.kind() == "annotation_ident")
        .map(|n| &source[n.start_byte()..n.end_byte()])?;
    if !matches!(annotation_name, "@wrapMethod" | "@replaceMethod") {
        return None;
    }
    annotation
        .children(&mut annotation.walk())
        .find(|c| c.kind() == "ident")
        .map(|n| &source[n.start_byte()..n.end_byte()])
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
