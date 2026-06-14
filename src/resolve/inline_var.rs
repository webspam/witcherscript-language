use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::SymbolKind;

use super::Definition;
use super::ast::{identifier_at, nodes_at_offset};
use super::definition::{definition_key, resolve_definition_at_byte};
use super::extract_common::{Splice, WriteSite, write_site_node, write_sites};
use super::references::find_references;
use super::symbol_db::SymbolDb;

pub enum InlineScope {
    AllUsages,
    SingleUsage,
}

pub struct Inlining {
    /// Non-overlapping edits against the original source
    pub edits: Vec<Splice>,
    pub scope: InlineScope,
}

// Forms safe to substitute as-is. Everything else is wrapped in parentheses so the operators
// around the substitution cannot change its value.
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
    let cursor_ident = identifier_at(root, byte_offset);

    // The `var` keyword has no identifier to resolve, so anchor on the declaration head instead.
    let anchor = match cursor_ident {
        Some(_) => byte_offset,
        None => decl_head_name(root, byte_offset)?.start_byte(),
    };
    let (def, decl) = variable_decl_at(uri, document, db, root, anchor)?;
    let plan = plan_inline(uri, document, db, root, &def, decl)?;

    let on_declaration = def.symbol.selection_byte_range.start <= byte_offset
        && byte_offset <= def.symbol.selection_byte_range.end;

    match cursor_ident {
        Some(ident) if !on_declaration => inline_single_read(ident, &plan),
        _ => Some(inline_all_reads(&plan)),
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

fn decl_head_name(root: Node<'_>, byte_offset: usize) -> Option<Node<'_>> {
    let decl = nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|node| find_ancestor_of_kind(node, &[kinds::LOCAL_VAR_DECL_STMT]))?;
    let name = single_name(decl)?;
    (decl.start_byte()..=name.end_byte())
        .contains(&byte_offset)
        .then_some(name)
}

struct InlinePlan {
    /// Substitution text for each read, parenthesised where precedence needs it
    value: String,
    reads: Vec<Range<usize>>,
    /// Splices that delete the declaration and any assignment that set the variable.
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
            resolve_definition_at_byte(uri, document, db, probe)
                .is_some_and(|d| definition_key(&d) == key)
        })
        .collect();
    let write_ranges: Vec<Range<usize>> = mutations
        .iter()
        .map(|w| write_site_node(w).byte_range())
        .collect();

    let decl_names = name_nodes(decl);
    let target_index = decl_names
        .iter()
        .position(|n| n.byte_range() == def.symbol.selection_byte_range)?;
    let reads = find_reads(uri, document, db, def, &write_ranges);

    let source = value_source(decl, &decl_names, &mutations, &reads)?;

    let mut teardown = vec![remove_binding(
        &document.source,
        decl,
        target_index,
        &decl_names,
    )];
    if let Some(assign_stmt) = source.defining_assignment {
        teardown.push(delete_statement(&document.source, assign_stmt));
    }

    Some(InlinePlan {
        value: substituted_text(&document.source, source.value_node),
        reads,
        teardown,
    })
}

struct ValueSource<'tree> {
    value_node: Node<'tree>,
    /// The assignment that set the value, deleted alongside the declaration.
    defining_assignment: Option<Node<'tree>>,
}

// `None` when the variable is not safely inlinable: reassigned after its initializer, or set by no
// single assignment that reaches every read.
fn value_source<'tree>(
    decl: Node<'tree>,
    decl_names: &[Node<'tree>],
    mutations: &[&WriteSite<'tree>],
    reads: &[Range<usize>],
) -> Option<ValueSource<'tree>> {
    // A list shares one initializer, so it cannot be the value for just one of the names.
    let initializer = (decl_names.len() == 1)
        .then(|| decl.child_by_field_name(fields::INIT_VALUE))
        .flatten();
    if let Some(init) = initializer {
        return mutations.is_empty().then_some(ValueSource {
            value_node: init,
            defining_assignment: None,
        });
    }

    if mutations.len() != 1 {
        return None;
    }
    let WriteSite::AssignTarget(assign_target) = mutations[0] else {
        return None;
    };
    let assign = direct_assign_expr(*assign_target)?;
    let assign_stmt = find_ancestor_of_kind(assign, &[kinds::EXPR_STMT])?;
    if !assignment_reaches_reads(assign_stmt, decl, reads) {
        return None;
    }
    Some(ValueSource {
        value_node: assign.child_by_field_name(fields::RIGHT)?,
        defining_assignment: Some(assign_stmt),
    })
}

// An unconditional sibling of the declaration that precedes every read reaches them all.
fn assignment_reaches_reads(assign_stmt: Node, decl: Node, reads: &[Range<usize>]) -> bool {
    let (Some(assign_parent), Some(decl_parent)) = (assign_stmt.parent(), decl.parent()) else {
        return false;
    };
    assign_parent.id() == decl_parent.id()
        && reads.iter().all(|r| r.start >= assign_stmt.end_byte())
}

// Writes must not be substituted with the value, so drop occurrences that land on a mutation site.
fn find_reads(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    def: &Definition,
    write_ranges: &[Range<usize>],
) -> Vec<Range<usize>> {
    find_references(def, document, &[(uri, document)], db, false)
        .into_iter()
        .filter_map(|(_, range)| {
            let start = document
                .line_index
                .position_to_byte(&document.source, range.start)?;
            let end = document
                .line_index
                .position_to_byte(&document.source, range.end)?;
            (!write_ranges.contains(&(start..end))).then_some(start..end)
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
    let scope = if plan.reads.len() > 1 {
        InlineScope::AllUsages
    } else {
        InlineScope::SingleUsage
    };
    Inlining { edits, scope }
}

fn inline_single_read(occurrence: Node, plan: &InlinePlan) -> Option<Inlining> {
    let range = occurrence.byte_range();
    if !plan.reads.contains(&range) {
        return None;
    }
    let mut edits = vec![Splice {
        range,
        text: plan.value.clone(),
    }];
    if plan.reads.len() == 1 {
        edits.extend(plan.teardown.iter().cloned());
    }
    Some(Inlining {
        edits,
        scope: InlineScope::SingleUsage,
    })
}

fn decl_stmt_for<'tree>(root: Node<'tree>, def: &Definition) -> Option<Node<'tree>> {
    let range = &def.symbol.byte_range;
    let node = root.descendant_for_byte_range(range.start, range.end)?;
    find_ancestor_of_kind(node, &[kinds::LOCAL_VAR_DECL_STMT])
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

fn remove_binding(source: &str, decl: Node, target_index: usize, names: &[Node]) -> Splice {
    match names {
        [_] => delete_statement(source, decl),
        _ => remove_name_from_list(target_index, names),
    }
}

fn remove_name_from_list(index: usize, names: &[Node]) -> Splice {
    let target = names[index];
    // Account for the comma we need to remove
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
        // Other code shares the statement's line, so keep that code and its indentation.
        start = stmt.start_byte();
    }

    Splice {
        range: start..end,
        text: String::new(),
    }
}
