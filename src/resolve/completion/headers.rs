use tree_sitter::Node;

use crate::cst::ancestors::node_and_ancestors;
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;

use super::super::MAX_INHERITANCE_DEPTH;
use super::super::ast::nodes_at_offset;
use super::super::symbol_db::SymbolDb;

pub fn class_header_keyword_completions(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Vec<&'static str> {
    let Some(header) = header_state_and_kind(document, position) else {
        return Vec::new();
    };
    match header.state {
        HeaderState::AfterClassName | HeaderState::AfterOwner => vec!["extends"],
        HeaderState::AfterStateName => vec!["in"],
        _ => Vec::new(),
    }
}

pub(super) fn class_chain(db: &SymbolDb, start: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut current = start.to_string();
    for _ in 0..=MAX_INHERITANCE_DEPTH {
        if chain.iter().any(|c| c == &current) {
            break;
        }
        chain.push(current.clone());
        match db.superclass_of(&current) {
            Some(next) => current = next,
            None => break,
        }
    }
    chain
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum HeaderState {
    Initial,
    AfterClassKw,
    AfterClassName,
    AfterStateKw,
    AfterStateName,
    AfterInKw,
    AfterOwner,
    AfterExtendsKw,
    AfterBase,
    Body,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum HeaderDeclKind {
    Class,
    State,
}

pub(super) struct HeaderContext {
    pub(super) state: HeaderState,
    pub(super) kind: Option<HeaderDeclKind>,
    pub(super) self_name: Option<String>,
    pub(super) owner_name: Option<String>,
}

pub(super) fn header_state_and_kind(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<HeaderContext> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let root = document.tree.root_node();

    let direct: Vec<Node> = nodes_at_offset(root, byte_offset)
        .into_iter()
        .filter(|n| n.kind() != "script")
        .collect();

    let header_node = direct
        .iter()
        .find_map(|n| enclosing_header_node(*n))
        .or_else(|| {
            let mut tc = root.walk();
            root.children(&mut tc)
                .take_while(|c| c.end_byte() <= byte_offset)
                .last()
                .and_then(enclosing_header_node)
        })?;

    let mut ctx = HeaderContext {
        state: HeaderState::Initial,
        kind: None,
        self_name: None,
        owner_name: None,
    };
    header_walk(
        header_node,
        byte_offset,
        document.source.as_bytes(),
        &mut ctx,
    );
    Some(ctx)
}

fn enclosing_header_node(start: Node) -> Option<Node> {
    node_and_ancestors(start).find_map(|current| match current.kind() {
        "class_decl" | "state_decl" => Some(current),
        "ERROR" => {
            if let Some(p) = current.parent()
                && matches!(p.kind(), "class_decl" | "state_decl")
            {
                return Some(p);
            }
            if node_contains_kind_any(current, &["class", "state"]) {
                Some(current)
            } else {
                None
            }
        }
        _ => None,
    })
}

fn header_walk(node: Node, byte_offset: usize, source: &[u8], ctx: &mut HeaderContext) {
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        if child.start_byte() >= byte_offset {
            break;
        }
        let past = child.end_byte() < byte_offset;
        match (ctx.state, child.kind()) {
            (HeaderState::Initial, "class") => {
                ctx.state = HeaderState::AfterClassKw;
                ctx.kind = Some(HeaderDeclKind::Class);
            }
            (HeaderState::Initial, "state") => {
                ctx.state = HeaderState::AfterStateKw;
                ctx.kind = Some(HeaderDeclKind::State);
            }
            (HeaderState::AfterClassKw, "ident") if past => {
                ctx.state = HeaderState::AfterClassName;
                ctx.self_name = child.utf8_text(source).ok().map(str::to_string);
            }
            (HeaderState::AfterStateKw, "ident") if past => {
                ctx.state = HeaderState::AfterStateName;
                ctx.self_name = child.utf8_text(source).ok().map(str::to_string);
            }
            (HeaderState::AfterStateName, "in") => ctx.state = HeaderState::AfterInKw,
            (HeaderState::AfterInKw, "ident") if past => {
                ctx.state = HeaderState::AfterOwner;
                ctx.owner_name = child.utf8_text(source).ok().map(str::to_string);
            }
            (HeaderState::AfterClassName | HeaderState::AfterOwner, "extends") => {
                ctx.state = HeaderState::AfterExtendsKw;
            }
            (HeaderState::AfterExtendsKw, "ident") if past => ctx.state = HeaderState::AfterBase,
            (_, "class_def" | "{") => ctx.state = HeaderState::Body,
            (_, "ERROR") => header_walk(child, byte_offset, source, ctx),
            _ => {}
        }
    }
}

fn node_contains_kind_any(node: Node, kinds: &[&str]) -> bool {
    let mut cursor = node.walk();

    node.children(&mut cursor)
        .any(|c| kinds.contains(&c.kind()))
}
