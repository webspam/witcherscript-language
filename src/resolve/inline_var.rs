use std::collections::{HashMap, HashSet};
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::{enclosing_callable_block, find_ancestor_of_kind};
use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::SymbolKind;

use super::Definition;
use super::ast::{identifier_at, nodes_at_offset};
use super::definition::{definition_key, resolve_definition_at_byte};
use super::extract_common::{Splice, WriteSite, write_site_node, write_sites};
use super::reaching_defs::{Def, reaching_defs};
use super::references::find_references;
use super::symbol_db::SymbolDb;

// Statements whose removal would drop an observable effect.
const SIDE_EFFECT_KINDS: &[&str] = &[kinds::FUNC_CALL_EXPR, kinds::NEW_EXPR];

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
        _ => inline_all_reads(&plan),
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
    /// Substitution text per read range, parenthesised where precedence needs it. Only reads with a
    /// single reaching definition whose value is stable to move appear here.
    eligible: Vec<(Range<usize>, String)>,
    total_reads: usize,
    /// Splices that delete the declaration and every assignment to the variable.
    teardown: Vec<Splice>,
    /// Whether teardown can run without dropping a side effect or leaving an undeletable assignment.
    teardown_safe: bool,
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
    let body = enclosing_callable_block(decl)?;
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
    if reads.is_empty() {
        return None;
    }

    let rd = reaching_defs(body, decl, decl_names.len(), &mutations, &reads);
    let write_positions = assigned_local_positions(uri, document, db, &all_writes);

    let mut eligible = Vec::new();
    let mut used = HashSet::new();
    for (range, sole) in &rd.per_read {
        let Some(idx) = sole else { continue };
        let def = &rd.all_defs[*idx];
        let Some(value) = def.value else {
            continue;
        };
        let captured_at = def.stmt.unwrap_or(decl).start_byte();
        if !operands_stable(value, uri, document, db, &write_positions, captured_at) {
            continue;
        }
        eligible.push((range.clone(), substituted_text(&document.source, value)));
        used.insert(*idx);
    }

    Some(InlinePlan {
        teardown: build_teardown(
            &document.source,
            decl,
            target_index,
            &decl_names,
            &rd.all_defs,
        ),
        teardown_safe: teardown_is_safe(&rd.all_defs, decl, &used),
        eligible,
        total_reads: reads.len(),
    })
}

// Assignment start bytes per local/parameter, keyed by its definition.
fn assigned_local_positions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    all_writes: &[WriteSite<'_>],
) -> HashMap<(String, Range<usize>), Vec<usize>> {
    let mut positions: HashMap<(String, Range<usize>), Vec<usize>> = HashMap::new();
    for write in all_writes {
        let node = write_site_node(write);
        if let Some(d) = resolve_definition_at_byte(uri, document, db, node.start_byte()) {
            positions
                .entry(definition_key(&d))
                .or_default()
                .push(node.start_byte());
        }
    }
    positions
}

// A write before `captured_at` already shaped the value; only writes at or after it can change an
// operand before the read, so they block the substitution.
fn operands_stable(
    value: Node<'_>,
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    write_positions: &HashMap<(String, Range<usize>), Vec<usize>>,
    captured_at: usize,
) -> bool {
    let mut idents = Vec::new();
    collect_descendants_of_kind(value, &[kinds::IDENT], &mut idents);
    idents.iter().all(|ident| {
        let Some(d) = resolve_definition_at_byte(uri, document, db, ident.start_byte()) else {
            return true;
        };
        if !matches!(d.symbol.kind, SymbolKind::Variable | SymbolKind::Parameter) {
            return true;
        }
        match write_positions.get(&definition_key(&d)) {
            None => true,
            Some(positions) => positions.iter().all(|&p| p < captured_at),
        }
    })
}

// Teardown deletes every definition; preserving one is unsafe when removing it would drop a side
// effect that was not relocated into a read, or when it has no deletable statement.
fn teardown_is_safe(all_defs: &[Def<'_>], decl: Node<'_>, used: &HashSet<usize>) -> bool {
    all_defs.iter().enumerate().all(|(i, def)| {
        let node = if def.is_decl { Some(decl) } else { def.stmt };
        match node {
            None => false,
            Some(node) => used.contains(&i) || !has_descendant_of_kind(node, SIDE_EFFECT_KINDS),
        }
    })
}

fn build_teardown(
    source: &str,
    decl: Node<'_>,
    target_index: usize,
    decl_names: &[Node<'_>],
    all_defs: &[Def<'_>],
) -> Vec<Splice> {
    let mut teardown = vec![remove_binding(source, decl, target_index, decl_names)];
    let mut seen = HashSet::from([decl.id()]);
    for stmt in all_defs
        .iter()
        .filter(|d| !d.is_decl)
        .filter_map(|d| d.stmt)
    {
        if seen.insert(stmt.id()) {
            teardown.push(delete_statement(source, stmt));
        }
    }
    teardown
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

fn inline_all_reads(plan: &InlinePlan) -> Option<Inlining> {
    if plan.eligible.len() != plan.total_reads || !plan.teardown_safe {
        return None;
    }
    let mut edits: Vec<Splice> = plan
        .eligible
        .iter()
        .map(|(range, text)| Splice {
            range: range.clone(),
            text: text.clone(),
        })
        .collect();
    edits.extend(plan.teardown.iter().cloned());
    let scope = if plan.total_reads > 1 {
        InlineScope::AllUsages
    } else {
        InlineScope::SingleUsage
    };
    Some(Inlining { edits, scope })
}

fn inline_single_read(occurrence: Node, plan: &InlinePlan) -> Option<Inlining> {
    let range = occurrence.byte_range();
    let (_, text) = plan.eligible.iter().find(|(r, _)| *r == range)?;
    let mut edits = vec![Splice {
        range,
        text: text.clone(),
    }];
    if plan.total_reads == 1 {
        if !plan.teardown_safe {
            return None;
        }
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
