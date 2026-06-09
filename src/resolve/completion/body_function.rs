use tree_sitter::Node;

use crate::cst::ancestors::node_and_ancestors;
use crate::cst::grammar::DEFAULT_OR_HINT_ASSIGN_KINDS;
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
            if ancestor.kind() == "member_default_val_block" {
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
        "ident", "var", "this", "super", "if", "else", "do", "while", "for", "switch", "return",
        "case", "default",
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
        .any(|n| nearest_enclosing_block(*n).is_some_and(|b| b.kind() == "switch_block"));

    let in_loop = nodes
        .iter()
        .any(|n| find_ancestor_of_kind(*n, &["for_stmt", "while_stmt", "do_while_stmt"]).is_some());

    Some(StatementCompletions {
        active: true,
        locals: base.locals,
        members: base.members,
        needs_globals: base.needs_globals,
        has_this: base.has_this,
        has_super: base.has_super,
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
            | "assign_op_direct"
            | "assign_op_sum"
            | "assign_op_diff"
            | "assign_op_mult"
            | "assign_op_div"
            | "assign_op_bitand"
            | "assign_op_bitor"
            | "binary_op_or"
            | "binary_op_and"
            | "binary_op_bitor"
            | "binary_op_bitand"
            | "binary_op_bitxor"
            | "binary_op_eq"
            | "binary_op_neq"
            | "binary_op_gt"
            | "binary_op_ge"
            | "binary_op_lt"
            | "binary_op_le"
            | "binary_op_diff"
            | "binary_op_sum"
            | "binary_op_mod"
            | "binary_op_div"
            | "binary_op_mult"
            | "unary_op_neg"
            | "unary_op_not"
            | "unary_op_bitnot"
            | "unary_op_plus"
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
        &["ident"],
    )?;

    Some(ExpressionCompletions {
        locals: base.locals,
        members: base.members,
        needs_globals: base.needs_globals,
        has_this: base.has_this,
        has_super: base.has_super,
    })
}

struct FunctionBodyContext {
    locals: Vec<Definition>,
    members: Vec<Definition>,
    needs_globals: bool,
    has_this: bool,
    has_super: bool,
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
        .any(|n| find_ancestor_of_kind(*n, &["func_block"]).is_some())
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
        },
    ))
}
