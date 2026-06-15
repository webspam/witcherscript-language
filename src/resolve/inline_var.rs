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
use super::extract_common::{Splice, WriteSite, delete_statement, write_site_node, write_sites};
use super::name_context::{NameContext, classify_ident_context};
use super::reaching_defs::{LocalDefinition, reaching_defs};
use super::references::find_references;
use super::symbol_db::SymbolDb;

// Statements whose removal would drop an observable effect.
const SIDE_EFFECT_KINDS: &[&str] = &[kinds::FUNC_CALL_EXPR, kinds::NEW_EXPR];

pub enum InlineScope {
    AllUsages,
    SingleUsage,
}

pub enum InlineConfidence {
    /// A single definition reaches each read and nothing the value depends on changes before it
    Verified,
    /// We cannot verify with certainty that there will be no runtime changes from inlining
    Unverified,
}

pub struct Inlining {
    /// Non-overlapping edits against the original source
    pub edits: Vec<Splice>,
    pub scope: InlineScope,
    pub confidence: InlineConfidence,
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

struct EligibleRead {
    range: Range<usize>,
    /// Substitution text, parenthesised where precedence needs it.
    text: String,
    /// The value is proven stable to move to this read.
    verified: bool,
}

struct InlinePlan {
    /// Reads with a single reaching definition that has a value not referencing the variable itself.
    eligible: Vec<EligibleRead>,
    total_reads: usize,
    /// Splices that delete the declaration and every assignment to the variable.
    teardown: Vec<Splice>,
    /// Teardown can produce a valid edit: every assignment has a deletable statement.
    teardown_possible: bool,
    /// Teardown drops no observable side effect.
    teardown_clean: bool,
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
    let ResolvedWrites {
        mutations,
        positions: write_positions,
    } = resolve_writes(uri, document, db, &all_writes, &key);
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

    let mut eligible = Vec::new();
    let mut used = HashSet::new();
    for (range, sole) in &rd.per_read {
        let Some(idx) = sole else { continue };
        let def = &rd.all_defs[*idx];
        let Some(value) = def.value else {
            continue;
        };
        let captured_at = def.stmt.unwrap_or(decl).start_byte();
        let verified = match check_operands(
            value,
            uri,
            document,
            db,
            &write_positions,
            captured_at,
            &key,
        ) {
            // Inlining would reference the variable the teardown removes.
            OperandCheck::ReferencesTarget => continue,
            OperandCheck::MayChange => false,
            OperandCheck::Stable => true,
        };
        let read_node = root.descendant_for_byte_range(range.start, range.end);
        eligible.push(EligibleRead {
            range: range.clone(),
            text: substituted_text(&document.source, value, read_node),
            verified,
        });
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
        teardown_possible: teardown_possible(&rd.all_defs),
        teardown_clean: teardown_clean(&rd.all_defs, decl, &used),
        eligible,
        total_reads: reads.len(),
    })
}

struct ResolvedWrites<'a, 't> {
    mutations: Vec<&'a WriteSite<'t>>,
    /// Assignment start bytes per local/parameter, keyed by its definition.
    positions: HashMap<(String, Range<usize>), Vec<usize>>,
}

fn resolve_writes<'a, 't>(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    all_writes: &'a [WriteSite<'t>],
    target: &(String, Range<usize>),
) -> ResolvedWrites<'a, 't> {
    let mut mutations = Vec::new();
    let mut positions: HashMap<(String, Range<usize>), Vec<usize>> = HashMap::new();
    for write in all_writes {
        let node = write_site_node(write);
        let Some(d) = resolve_definition_at_byte(uri, document, db, node.start_byte()) else {
            continue;
        };
        let key = definition_key(&d);
        if key == *target {
            mutations.push(write);
        }
        positions.entry(key).or_default().push(node.start_byte());
    }
    ResolvedWrites {
        mutations,
        positions,
    }
}

enum OperandCheck {
    Stable,
    MayChange,
    ReferencesTarget,
}

fn check_operands(
    value: Node<'_>,
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    write_positions: &HashMap<(String, Range<usize>), Vec<usize>>,
    captured_at: usize,
    target: &(String, Range<usize>),
) -> OperandCheck {
    let mut idents = Vec::new();
    collect_descendants_of_kind(value, &[kinds::IDENT], &mut idents);
    let source = document.source.as_bytes();
    let mut stable = true;
    for ident in idents {
        // Callee names, member names, and type names do not carry a value whose stability matters.
        if classify_ident_context(ident, source) != Some(NameContext::Value) {
            continue;
        }
        let Some(d) = resolve_definition_at_byte(uri, document, db, ident.start_byte()) else {
            // An unresolved value reference cannot be checked, so the value is not verifiable.
            stable = false;
            continue;
        };
        if !matches!(d.symbol.kind, SymbolKind::Variable | SymbolKind::Parameter) {
            continue;
        }
        let key = definition_key(&d);
        if key == *target {
            return OperandCheck::ReferencesTarget;
        }
        // Only writes at or after the defining statement can change an operand before the read.
        if let Some(positions) = write_positions.get(&key)
            && positions.iter().any(|&p| p >= captured_at)
        {
            stable = false;
        }
    }
    if stable {
        OperandCheck::Stable
    } else {
        OperandCheck::MayChange
    }
}

fn teardown_possible(all_defs: &[LocalDefinition<'_>]) -> bool {
    all_defs
        .iter()
        .filter(|d| !d.is_decl)
        .all(|d| d.stmt.is_some())
}

fn teardown_clean(all_defs: &[LocalDefinition<'_>], decl: Node<'_>, used: &HashSet<usize>) -> bool {
    all_defs.iter().enumerate().all(|(i, def)| {
        // A used store's value moved into a read, so dropping its statement keeps the effect.
        if used.contains(&i) {
            return true;
        }
        let node = if def.is_decl { Some(decl) } else { def.stmt };
        node.is_none_or(|n| !has_descendant_of_kind(n, SIDE_EFFECT_KINDS))
    })
}

fn build_teardown(
    source: &str,
    decl: Node<'_>,
    target_index: usize,
    decl_names: &[Node<'_>],
    all_defs: &[LocalDefinition<'_>],
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

fn confidence(verified: bool) -> InlineConfidence {
    if verified {
        InlineConfidence::Verified
    } else {
        InlineConfidence::Unverified
    }
}

fn inline_all_reads(plan: &InlinePlan) -> Option<Inlining> {
    if plan.eligible.len() != plan.total_reads || !plan.teardown_possible {
        return None;
    }
    let mut edits: Vec<Splice> = plan
        .eligible
        .iter()
        .map(|read| Splice {
            range: read.range.clone(),
            text: read.text.clone(),
        })
        .collect();
    edits.extend(plan.teardown.iter().cloned());
    let scope = if plan.total_reads > 1 {
        InlineScope::AllUsages
    } else {
        InlineScope::SingleUsage
    };
    let verified = plan.teardown_clean && plan.eligible.iter().all(|read| read.verified);
    Some(Inlining {
        edits,
        scope,
        confidence: confidence(verified),
    })
}

fn inline_single_read(occurrence: Node, plan: &InlinePlan) -> Option<Inlining> {
    let range = occurrence.byte_range();
    let read = plan.eligible.iter().find(|read| read.range == range)?;
    let mut edits = vec![Splice {
        range,
        text: read.text.clone(),
    }];
    let mut verified = read.verified;
    if plan.total_reads == 1 {
        if !plan.teardown_possible {
            return None;
        }
        edits.extend(plan.teardown.iter().cloned());
        verified = verified && plan.teardown_clean;
    }
    Some(Inlining {
        edits,
        scope: InlineScope::SingleUsage,
        confidence: confidence(verified),
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

fn substituted_text(source: &str, value: Node, read: Option<Node>) -> String {
    let text = &source[value.byte_range()];
    if ATOMIC_INIT_KINDS.contains(&value.kind()) || !context_binds_tighter(read) {
        text.to_string()
    } else {
        format!("({text})")
    }
}

fn context_binds_tighter(read: Option<Node>) -> bool {
    let Some(parent) = read.and_then(|r| r.parent()) else {
        return false;
    };
    // In these positions the value is a whole operand, so no outer operator can capture part of it.
    !matches!(
        parent.kind(),
        kinds::RETURN_STMT
            | kinds::EXPR_STMT
            | kinds::FUNC_CALL_ARGS
            | kinds::NESTED_EXPR
            | kinds::LOCAL_VAR_DECL_STMT
            | kinds::ASSIGN_OP_EXPR
            | kinds::SWITCH_CASE_LABEL
            | kinds::DELETE_STMT
            | kinds::SEQUENCE_EXPRESSION
    )
}
