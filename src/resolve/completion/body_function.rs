use tree_sitter::Node;

use crate::cst::ancestors::node_and_ancestors;
use crate::cst::grammar::DEFAULT_OR_HINT_ASSIGN_KINDS;
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{AccessLevel, SymbolKind};

use super::super::Definition;
use super::super::ast::{
    find_ancestor_of_kind, is_kind_or_error_wrapped_kind, is_statement_boundary,
    nearest_enclosing_block, nodes_at_offset, significant_node_before_byte,
};
use super::super::inference::enclosing_type_context;
use super::super::symbol_db::SymbolDb;

pub fn default_or_hint_member_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    let Some(byte_offset) = document
        .line_index
        .position_to_byte(&document.source, position)
    else {
        return Vec::new();
    };
    let root = document.tree.root_node();

    if !at_default_or_hint_member_pos(root, byte_offset) {
        return Vec::new();
    }

    let Some(enclosing) = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
    ) else {
        return Vec::new();
    };

    db.members_of(&enclosing.name, AccessLevel::Private)
}

fn at_default_or_hint_member_pos(root: Node, byte_offset: usize) -> bool {
    for n in nodes_at_offset(root, byte_offset) {
        for ancestor in node_and_ancestors(n) {
            if DEFAULT_OR_HINT_ASSIGN_KINDS.contains(&ancestor.kind()) {
                return cursor_before_eq(ancestor, byte_offset);
            }
            if ancestor.kind() == kinds::MEMBER_DEFAULT_VAL_BLOCK {
                return true;
            }
        }
    }
    false
}

fn cursor_before_eq(assign: Node, byte_offset: usize) -> bool {
    let mut cursor = assign.walk();
    let eq = assign.children(&mut cursor).find(|c| c.kind() == "=");
    match eq {
        Some(eq) => byte_offset <= eq.start_byte(),
        None => true,
    }
}

pub struct StatementCompletions {
    pub active: bool,
    pub locals: Vec<Definition>,
    pub members: Vec<Definition>,
    pub needs_globals: bool,
    pub has_this: bool,
    pub has_super: bool,
    pub has_parent: bool,
    pub in_switch: bool,
    pub in_loop: bool,
}

pub fn statement_completions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> StatementCompletions {
    statement_completions_inner(uri, document, db, position).unwrap_or(StatementCompletions {
        active: false,
        locals: vec![],
        members: vec![],
        needs_globals: false,
        has_this: false,
        has_super: false,
        has_parent: false,
        in_switch: false,
        in_loop: false,
    })
}

fn statement_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<StatementCompletions> {
    const STMT_WRITER_KINDS: &[&str] = &[
        kinds::IDENT,
        "var",
        "this",
        "super",
        "parent",
        "virtual_parent",
        "if",
        "else",
        "do",
        "while",
        "for",
        "switch",
        "return",
        "case",
        "default",
    ];
    let (nodes, base) = function_body_completions(
        uri,
        document,
        db,
        position,
        is_statement_boundary,
        STMT_WRITER_KINDS,
    )?;

    let in_switch = nodes
        .iter()
        .any(|n| nearest_enclosing_block(*n).is_some_and(|b| b.kind() == kinds::SWITCH_BLOCK));

    let in_loop = nodes.iter().any(|n| {
        find_ancestor_of_kind(
            *n,
            &[kinds::FOR_STMT, kinds::WHILE_STMT, kinds::DO_WHILE_STMT],
        )
        .is_some()
    });

    Some(StatementCompletions {
        active: true,
        locals: base.locals,
        members: base.members,
        needs_globals: base.needs_globals,
        has_this: base.has_this,
        has_super: base.has_super,
        has_parent: base.has_parent,
        in_switch,
        in_loop,
    })
}

pub struct ExpressionCompletions {
    pub locals: Vec<Definition>,
    pub members: Vec<Definition>,
    pub needs_globals: bool,
    pub has_this: bool,
    pub has_super: bool,
    pub has_parent: bool,
}

pub fn expression_completions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<ExpressionCompletions> {
    expression_completions_inner(uri, document, db, position)
}

fn is_expression_boundary(node: Node) -> bool {
    matches!(
        node.kind(),
        "(" | ","
            | "="
            | "return"
            | kinds::ASSIGN_OP_DIRECT
            | kinds::ASSIGN_OP_SUM
            | kinds::ASSIGN_OP_DIFF
            | kinds::ASSIGN_OP_MULT
            | kinds::ASSIGN_OP_DIV
            | kinds::ASSIGN_OP_BITAND
            | kinds::ASSIGN_OP_BITOR
            | kinds::BINARY_OP_OR
            | kinds::BINARY_OP_AND
            | kinds::BINARY_OP_BITOR
            | kinds::BINARY_OP_BITAND
            | kinds::BINARY_OP_BITXOR
            | kinds::BINARY_OP_EQ
            | kinds::BINARY_OP_NEQ
            | kinds::BINARY_OP_GT
            | kinds::BINARY_OP_GE
            | kinds::BINARY_OP_LT
            | kinds::BINARY_OP_LE
            | kinds::BINARY_OP_DIFF
            | kinds::BINARY_OP_SUM
            | kinds::BINARY_OP_MOD
            | kinds::BINARY_OP_DIV
            | kinds::BINARY_OP_MULT
            | kinds::UNARY_OP_NEG
            | kinds::UNARY_OP_NOT
            | kinds::UNARY_OP_BITNOT
            | kinds::UNARY_OP_PLUS
    )
}

fn expression_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<ExpressionCompletions> {
    let (_, base) = function_body_completions(
        uri,
        document,
        db,
        position,
        is_expression_boundary,
        &[kinds::IDENT],
    )?;

    Some(ExpressionCompletions {
        locals: base.locals,
        members: base.members,
        needs_globals: base.needs_globals,
        has_this: base.has_this,
        has_super: base.has_super,
        has_parent: base.has_parent,
    })
}

struct FunctionBodyContext {
    locals: Vec<Definition>,
    members: Vec<Definition>,
    needs_globals: bool,
    has_this: bool,
    has_super: bool,
    has_parent: bool,
}

fn function_body_completions<'a>(
    uri: &str,
    document: &'a ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
    boundary: fn(Node) -> bool,
    writer_kinds: &[&str],
) -> Option<(Vec<Node<'a>>, FunctionBodyContext)> {
    const STMT_KEYWORD_KINDS: &[&str] = &[
        "if", "else", "var", "return", "do", "while", "for", "switch", "case", "default", "break",
        "continue",
    ];

    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);

    let prev = significant_node_before_byte(root, document.source.as_bytes(), byte_offset);
    let at_start = prev.is_some_and(boundary);
    let writing_first = nodes
        .last()
        .filter(|&n| is_kind_or_error_wrapped_kind(*n, writer_kinds))
        .and_then(|n| {
            significant_node_before_byte(root, document.source.as_bytes(), n.start_byte())
        })
        .is_some_and(boundary);
    if !at_start && !writing_first {
        return None;
    }

    if !nodes
        .iter()
        .any(|n| find_ancestor_of_kind(*n, &[kinds::FUNC_BLOCK]).is_some())
    {
        return None;
    }

    let callable = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    )?;

    let locals: Vec<Definition> = document
        .symbols
        .children_of(Some(callable.id))
        .filter(|sym| {
            matches!(sym.kind, SymbolKind::Variable | SymbolKind::Parameter)
                && sym.selection_byte_range.start <= byte_offset
        })
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
        .collect();

    let current_type = enclosing_type_context(document, db, byte_offset);
    let members: Vec<Definition> = current_type
        .as_ref()
        .map(|t| db.members_of(&t.name, AccessLevel::Private))
        .unwrap_or_default();
    let has_this = current_type.is_some();
    let has_super = current_type
        .as_ref()
        .and_then(|t| t.base_class.as_deref())
        .is_some();
    // States expose `parent`/`virtual_parent`, both resolving to the owner class.
    let has_parent = current_type
        .as_ref()
        .and_then(|t| t.owner_class.as_deref())
        .is_some();

    let writing_keyword = nodes
        .last()
        .is_some_and(|n| is_kind_or_error_wrapped_kind(*n, STMT_KEYWORD_KINDS));
    let needs_globals = (at_start || writing_first) && !writing_keyword;

    Some((
        nodes,
        FunctionBodyContext {
            locals,
            members,
            needs_globals,
            has_this,
            has_super,
            has_parent,
        },
    ))
}
