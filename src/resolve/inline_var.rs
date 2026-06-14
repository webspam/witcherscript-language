use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::grammar::{is_assignment_target, write_target};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::SymbolKind;

use super::Definition;
use super::ast::{identifier_at, nodes_at_offset};
use super::definition::{definition_key, resolve_definition_at_byte};
use super::extract_common::{Splice, WriteSite, out_args, write_site_node, write_sites};
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

    // On the `var` keyword there is no identifier; a declaration head still inlines all usages.
    let Some(cursor_ident) = identifier_at(root, byte_offset) else {
        let name = decl_head_name(root, byte_offset)?;
        let (def, decl) = variable_decl_at(uri, document, db, root, name.start_byte())?;
        let plan = plan_inline(uri, document, db, root, &def, decl)?;
        return Some(inline_all_reads(&plan));
    };

    let (def, decl) = variable_decl_at(uri, document, db, root, byte_offset)?;
    let plan = plan_inline(uri, document, db, root, &def, decl)?;

    // Inclusive: a cursor at the name's end byte is on the declaration, not a use.
    let on_declaration = def.symbol.selection_byte_range.start <= byte_offset
        && byte_offset <= def.symbol.selection_byte_range.end;

    if on_declaration {
        Some(inline_all_reads(&plan))
    } else {
        inline_single_read(uri, document, db, cursor_ident, &plan)
    }
}

fn variable_decl_at<'t>(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    root: Node<'t>,
    anchor_byte: usize,
) -> Option<(Definition, Node<'t>)> {
    let def = resolve_definition_at_byte(uri, document, db, anchor_byte)?;
    if def.symbol.kind != SymbolKind::Variable || def.uri.as_str() != uri {
        return None;
    }
    let decl = decl_stmt_for(root, &def)?;
    Some((def, decl))
}

// The `var` keyword through the declared name; a cursor here targets the declaration, not a use.
fn decl_head_name(root: Node<'_>, byte_offset: usize) -> Option<Node<'_>> {
    let decl = nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|node| {
            if node.kind() == kinds::LOCAL_VAR_DECL_STMT {
                Some(node)
            } else {
                find_ancestor_of_kind(node, &[kinds::LOCAL_VAR_DECL_STMT])
            }
        })?;
    let name = single_name(decl)?;
    (decl.start_byte()..=name.end_byte())
        .contains(&byte_offset)
        .then_some(name)
}

struct InlinePlan {
    /// Substitution text for each read, parenthesised where precedence needs it.
    value: String,
    reads: Vec<Range<usize>>,
    /// Edits removing the variable's binding once every read is inlined: its declaration (or just
    /// this name from a multi-name list), plus a defining assignment when separate.
    teardown: Vec<Splice>,
}

fn plan_inline(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    root: Node<'_>,
    def: &Definition,
    decl: Node<'_>,
) -> Option<InlinePlan> {
    let container = def.symbol.container?;
    let scope = document.symbols.by_id(container)?.byte_range.clone();
    let scope_node = root.descendant_for_byte_range(scope.start, scope.end)?;
    let key = definition_key(def);

    let all_writes = write_sites(uri, document, db, &[scope_node]);
    let mutations: Vec<&WriteSite> = all_writes
        .iter()
        .filter(|w| {
            let probe = write_site_node(w).start_byte();
            occurrence_resolves_to(uri, document, db, probe, std::slice::from_ref(&key))
        })
        .collect();

    let decl_names = name_nodes(decl);
    let target = decl_names
        .iter()
        .copied()
        .find(|n| n.byte_range() == def.symbol.selection_byte_range)?;
    let reads = read_occurrences(uri, document, db, def, decl, &scope);

    // A shared initializer belongs to a lone name only; a list assigns each variable separately.
    let initializer = (decl_names.len() == 1)
        .then(|| decl.child_by_field_name(fields::INIT_VALUE))
        .flatten();

    let mut defining_assignment = None;
    let value_node = if let Some(init) = initializer {
        // Initializer holds the value; inlinable only when nothing else mutates the variable.
        if !mutations.is_empty() {
            return None;
        }
        init
    } else {
        // Otherwise the value must come from exactly one direct `=` assignment of this variable.
        if mutations.len() != 1 {
            return None;
        }
        let WriteSite::AssignTarget(assign_target) = mutations[0] else {
            return None;
        };
        let assign = direct_assign_expr(*assign_target)?;
        let assign_stmt = find_ancestor_of_kind(assign, &[kinds::EXPR_STMT])?;
        // An unconditional sibling of the declaration that precedes every read dominates them all.
        if assign_stmt.parent()?.id() != decl.parent()?.id()
            || reads.iter().any(|r| r.start < assign_stmt.end_byte())
        {
            return None;
        }
        defining_assignment = Some(assign_stmt);
        assign.child_by_field_name(fields::RIGHT)?
    };

    let mut teardown = vec![remove_binding(&document.source, decl, target, &decl_names)];
    if let Some(assign_stmt) = defining_assignment {
        teardown.push(delete_statement(&document.source, assign_stmt));
    }

    Some(InlinePlan {
        value: substituted_text(&document.source, value_node),
        reads,
        teardown,
    })
}

fn read_occurrences(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    def: &Definition,
    decl: Node<'_>,
    scope: &Range<usize>,
) -> Vec<Range<usize>> {
    let root = document.tree.root_node();
    let key = definition_key(def);
    let mut occurrences = Vec::new();
    collect_ident_occurrences(
        root,
        document.source.as_bytes(),
        &def.symbol.name,
        Some(scope),
        &mut occurrences,
    );
    occurrences
        .into_iter()
        .filter(|occ| {
            // The declaration's own name (and its initializer) is not a use to replace.
            if decl.start_byte() <= occ.start && occ.start < decl.end_byte() {
                return false;
            }
            let Some(ident) = identifier_at(root, occ.start) else {
                return false;
            };
            if occurrence_is_write(uri, document, db, ident) {
                return false;
            }
            // The same name can reach an unrelated field via `obj.name`; keep only true references.
            occurrence_resolves_to(uri, document, db, occ.start, std::slice::from_ref(&key))
        })
        .collect()
}

fn direct_assign_expr(target: Node<'_>) -> Option<Node<'_>> {
    let assign = find_ancestor_of_kind(target, &[kinds::ASSIGN_OP_EXPR])?;
    let op = assign.child_by_field_name(fields::OP)?;
    (op.kind() == kinds::ASSIGN_OP_DIRECT).then_some(assign)
}

fn inline_all_reads(plan: &InlinePlan) -> Inlining {
    let mut edits: Vec<Splice> = plan
        .reads
        .iter()
        .map(|range| Splice {
            range: range.clone(),
            text: plan.value.clone(),
        })
        .collect();
    edits.extend(plan.teardown.iter().cloned());
    Inlining {
        edits,
        scope: InlineScope::AllUsages,
    }
}

fn inline_single_read(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    occurrence: Node,
    plan: &InlinePlan,
) -> Option<Inlining> {
    // A write target is an lvalue; replacing it with a value expression would not parse.
    if occurrence_is_write(uri, document, db, occurrence) {
        return None;
    }
    let mut edits = vec![Splice {
        range: occurrence.byte_range(),
        text: plan.value.clone(),
    }];
    // Inlining the final read leaves the variable's binding dead.
    let scope = if plan.reads.len() == 1 {
        edits.extend(plan.teardown.iter().cloned());
        InlineScope::AllUsages
    } else {
        InlineScope::SingleUsage
    };
    Some(Inlining { edits, scope })
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

fn name_nodes(decl: Node) -> Vec<Node> {
    let mut cursor = decl.walk();
    decl.children_by_field_name(fields::NAMES, &mut cursor)
        .filter(|n| n.kind() == kinds::IDENT)
        .collect()
}

fn single_name(decl: Node) -> Option<Node> {
    match name_nodes(decl).as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

// Whole statement for a lone name, else just this name spliced out of the `var a, b` list.
fn remove_binding(source: &str, decl: Node, target: Node, names: &[Node]) -> Splice {
    match names {
        [_] => delete_statement(source, decl),
        _ => remove_name_from_list(target, names),
    }
}

fn remove_name_from_list(target: Node, names: &[Node]) -> Splice {
    let index = names
        .iter()
        .position(|n| n.id() == target.id())
        .unwrap_or(0);
    // Drop the comma that joins this name to the rest: the trailing one for the first, else the leading.
    let range = if index == 0 {
        target.start_byte()..names[1].start_byte()
    } else {
        names[index - 1].end_byte()..target.end_byte()
    };
    Splice {
        range,
        text: String::new(),
    }
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

// Delete a statement with its line when it stands alone, so no blank line remains.
fn delete_statement(source: &str, stmt: Node) -> Splice {
    let bytes = source.as_bytes();
    let mut start = stmt.start_byte();
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    let at_line_start = start == 0 || bytes[start - 1] == b'\n';

    let mut end = stmt.end_byte();
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
        // Something precedes the statement on its line; keep that code and its indentation.
        start = stmt.start_byte();
    }

    Splice {
        range: start..end,
        text: String::new(),
    }
}
