use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::grammar::{is_assignment_target, write_target};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::SymbolKind;

use super::Definition;
use super::ast::identifier_at;
use super::definition::{definition_key, resolve_definition_at_byte};
use super::extract_common::{Splice, out_args};
use super::references::{collect_ident_occurrences, occurrence_resolves_to};
use super::symbol_db::SymbolDb;

/// Which uses an inline replaces.
pub enum InlineScope {
    /// Cursor on the declaration: replace every read, then delete the declaration.
    AllUsages,
    /// Cursor on one use: replace just that occurrence.
    SingleUsage,
}

pub struct Inlining {
    /// Non-overlapping edits against the original source.
    pub edits: Vec<Splice>,
    pub scope: InlineScope,
}

// Initializer forms that never need wrapping when substituted; everything else is parenthesised
// so surrounding operator precedence cannot change the substituted value.
const ATOMIC_INIT_KINDS: &[&str] = &[
    kinds::IDENT,
    kinds::LITERAL_INT,
    kinds::LITERAL_HEX,
    kinds::LITERAL_FLOAT,
    kinds::LITERAL_BOOL,
    kinds::LITERAL_STRING,
    kinds::LITERAL_NAME,
    kinds::LITERAL_NULL,
    kinds::FUNC_CALL_EXPR,
    kinds::MEMBER_ACCESS_EXPR,
    kinds::ARRAY_EXPR,
    kinds::NESTED_EXPR,
    kinds::NEW_EXPR,
    kinds::THIS_EXPR,
    kinds::PARENT_EXPR,
    kinds::SUPER_EXPR,
    kinds::VIRTUAL_PARENT_EXPR,
];

pub fn inline_variable(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<Inlining> {
    let root = document.tree.root_node();
    let cursor_ident = identifier_at(root, byte_offset)?;
    let def = resolve_definition_at_byte(uri, document, db, byte_offset)?;
    if def.symbol.kind != SymbolKind::Variable || def.uri.as_str() != uri {
        return None;
    }

    let decl = decl_stmt_for(root, &def)?;
    if name_count(decl) != 1 {
        // A multi-name `var a, b` declaration has no single statement to delete cleanly.
        return None;
    }
    // An uninitialised local has no value to substitute.
    let init = decl.child_by_field_name(fields::INIT_VALUE)?;
    let replacement = substituted_text(&document.source, init);

    // Inclusive: a cursor at the name's end byte is on the declaration, not a use.
    let on_declaration = def.symbol.selection_byte_range.start <= byte_offset
        && byte_offset <= def.symbol.selection_byte_range.end;

    if on_declaration {
        inline_all_usages(uri, document, db, &def, decl, &replacement)
    } else {
        inline_single_usage(uri, document, db, cursor_ident, replacement)
    }
}

fn inline_single_usage(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    occurrence: Node,
    replacement: String,
) -> Option<Inlining> {
    // A write target is an lvalue; replacing it with a value expression would not parse.
    if occurrence_is_write(uri, document, db, occurrence) {
        return None;
    }
    Some(Inlining {
        edits: vec![Splice {
            range: occurrence.byte_range(),
            text: replacement,
        }],
        scope: InlineScope::SingleUsage,
    })
}

fn inline_all_usages(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    def: &Definition,
    decl: Node,
    replacement: &str,
) -> Option<Inlining> {
    let container = def.symbol.container?;
    let scope = document.symbols.by_id(container)?.byte_range.clone();
    let root = document.tree.root_node();

    let mut occurrences = Vec::new();
    collect_ident_occurrences(
        root,
        document.source.as_bytes(),
        &def.symbol.name,
        Some(&scope),
        &mut occurrences,
    );

    let mut edits = Vec::new();
    for occ in occurrences {
        // The declaration (its name and initializer) is removed wholesale, not rewritten.
        if decl.start_byte() <= occ.start && occ.start < decl.end_byte() {
            continue;
        }
        let Some(ident) = identifier_at(root, occ.start) else {
            continue;
        };
        // The same name can reach an unrelated field via `obj.name`; inline only true references.
        if !occurrence_resolves_to(uri, document, db, occ.start, &[definition_key(def)]) {
            continue;
        }
        if occurrence_is_write(uri, document, db, ident) {
            // A reassigned variable's initializer is not its value at every use.
            return None;
        }
        edits.push(Splice {
            range: occ,
            text: replacement.to_string(),
        });
    }

    edits.push(delete_statement(&document.source, decl));
    Some(Inlining {
        edits,
        scope: InlineScope::AllUsages,
    })
}

fn decl_stmt_for<'tree>(root: Node<'tree>, def: &Definition) -> Option<Node<'tree>> {
    let range = &def.symbol.byte_range;
    let node = root.descendant_for_byte_range(range.start, range.end)?;
    if node.kind() == kinds::LOCAL_VAR_DECL_STMT {
        Some(node)
    } else {
        find_ancestor_of_kind(node, &[kinds::LOCAL_VAR_DECL_STMT])
    }
}

fn name_count(decl: Node) -> usize {
    let mut cursor = decl.walk();
    decl.children_by_field_name(fields::NAMES, &mut cursor)
        .filter(|n| n.kind() == kinds::IDENT)
        .count()
}

fn substituted_text(source: &str, init: Node) -> String {
    let text = &source[init.byte_range()];
    if ATOMIC_INIT_KINDS.contains(&init.kind()) {
        text.to_string()
    } else {
        format!("({text})")
    }
}

fn occurrence_is_write(uri: &str, document: &ParsedDocument, db: &SymbolDb, ident: Node) -> bool {
    if is_assignment_target(ident) {
        return true;
    }
    let Some(call) = find_ancestor_of_kind(ident, &[kinds::FUNC_CALL_EXPR]) else {
        return false;
    };
    out_args(uri, document, db, call)
        .iter()
        .any(|arg| write_target(*arg).map(|n| n.id()) == Some(ident.id()))
}

// Delete the declaration along with its line when it occupies one on its own, so no blank line remains.
fn delete_statement(source: &str, decl: Node) -> Splice {
    let bytes = source.as_bytes();
    let mut start = decl.start_byte();
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    let at_line_start = start == 0 || bytes[start - 1] == b'\n';

    let mut end = decl.end_byte();
    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t') {
        end += 1;
    }
    if at_line_start {
        if end < bytes.len() && bytes[end] == b'\r' {
            end += 1;
        }
        if end < bytes.len() && bytes[end] == b'\n' {
            end += 1;
        }
    } else {
        // Something precedes the declaration on its line; keep that code and its indentation.
        start = decl.start_byte();
    }

    Splice {
        range: start..end,
        text: String::new(),
    }
}
