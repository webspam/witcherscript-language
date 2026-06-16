use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::SymbolKind;

use super::Definition;
use super::ast::{identifier_at, nodes_at_offset};
use super::body_model::{BodyModel, ReachDef, Stability};
use super::definition::resolve_definition_at_byte;
use super::extract_common::{Splice, delete_statement};
use super::symbol_db::SymbolDb;

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
    let plan = plan_inline(uri, document, db, &def, decl)?;

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
    def: &Definition,
    decl: Node<'_>,
) -> Option<InlinePlan> {
    let anchor = def.symbol.selection_byte_range.start;
    let model = BodyModel::enclosing(uri, document, db, anchor)?;
    let target = model.local_declared_at(anchor)?;

    let decl_names = name_nodes(decl);
    let target_index = decl_names
        .iter()
        .position(|n| n.byte_range() == def.symbol.selection_byte_range)?;

    let reaching = model.reaching(target);
    if reaching.per_read().is_empty() {
        return None;
    }
    let defs = reaching.defs();

    let mut eligible = Vec::new();
    let mut used = HashSet::new();
    for (range, sole) in reaching.per_read() {
        let Some(idx) = sole else { continue };
        let Some(value) = defs[*idx].value() else {
            continue;
        };
        let captured_at = defs[*idx].stmt().map_or(decl.start_byte(), |s| s.start);
        let verified = match model.value_stability(&value, captured_at, target) {
            // Inlining would reference the variable the teardown removes.
            Stability::ReferencesTarget => continue,
            Stability::MayChange => false,
            Stability::Stable => true,
        };
        eligible.push(EligibleRead {
            range: range.clone(),
            text: substituted_text(&document.source, &value, range, &model),
            verified,
        });
        used.insert(*idx);
    }

    Some(InlinePlan {
        teardown: build_teardown(&document.source, decl, target_index, &decl_names, defs),
        teardown_possible: teardown_possible(defs),
        teardown_clean: teardown_clean(defs, decl, &used, &model),
        eligible,
        total_reads: reaching.per_read().len(),
    })
}

fn teardown_possible(defs: &[ReachDef]) -> bool {
    defs.iter()
        .filter(|d| !d.is_decl())
        .all(|d| d.stmt().is_some())
}

fn teardown_clean(
    defs: &[ReachDef],
    decl: Node<'_>,
    used: &HashSet<usize>,
    model: &BodyModel,
) -> bool {
    defs.iter().enumerate().all(|(i, def)| {
        // A used store's value moved into a read, so dropping its statement keeps the effect.
        if used.contains(&i) {
            return true;
        }
        let stmt = if def.is_decl() {
            Some(decl.byte_range())
        } else {
            def.stmt()
        };
        stmt.is_none_or(|s| !model.has_observable_effect(&s))
    })
}

fn build_teardown(
    source: &str,
    decl: Node<'_>,
    target_index: usize,
    decl_names: &[Node<'_>],
    defs: &[ReachDef],
) -> Vec<Splice> {
    let mut teardown = vec![remove_binding(source, decl, target_index, decl_names)];
    let mut seen = HashSet::from([decl.byte_range()]);
    for stmt in defs
        .iter()
        .filter(|d| !d.is_decl())
        .filter_map(ReachDef::stmt)
    {
        if seen.insert(stmt.clone()) {
            teardown.push(delete_statement(source, stmt));
        }
    }
    teardown
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
        [_] => delete_statement(source, decl.byte_range()),
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

fn substituted_text(
    source: &str,
    value: &Range<usize>,
    read: &Range<usize>,
    model: &BodyModel,
) -> String {
    let text = &source[value.clone()];
    if model.needs_parentheses(value, read) {
        format!("({text})")
    } else {
        text.to_string()
    }
}
