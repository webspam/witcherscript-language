use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::body_model::{BodyModel, Declaration, LocalId, ReachDef, Stability};
use super::edit_plan::{Confidence, EditPlan, Splice, delete_statement, remove_list_entry};

pub enum InlineScope {
    AllUsages,
    SingleUsage,
}

pub struct Inlining {
    pub plan: EditPlan,
    pub scope: InlineScope,
}

pub fn inline_variable(model: &BodyModel, byte_offset: usize) -> Option<Inlining> {
    let target = model.variable_at(byte_offset)?;
    let plan = plan_inline(model, target)?;
    match model.read_at(target, byte_offset) {
        Some(range) => inline_single_read(&range, &plan),
        None => inline_all_reads(&plan),
    }
}

struct EligibleRead {
    range: Range<usize>,
    /// Substitution text, parenthesised where precedence needs it.
    text: String,
    /// The value is proven stable to move to this read.
    verified: bool,
    calls_or_constructs: bool,
    def_index: usize,
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

fn plan_inline(model: &BodyModel, target: LocalId) -> Option<InlinePlan> {
    let source = &model.document().source;
    let decl = model.declaration(target)?;

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
        let captured_at = defs[*idx].stmt().map_or(decl.stmt.start, |s| s.start);
        let verified = match model.value_stability(&value, captured_at, target) {
            // Inlining would reference the variable the teardown removes.
            Stability::ReferencesTarget => continue,
            Stability::MayChange => false,
            Stability::Stable => true,
        };
        eligible.push(EligibleRead {
            range: range.clone(),
            text: substituted_text(source, &value, range, model),
            verified,
            calls_or_constructs: model.value_calls_or_constructs(&value),
            def_index: *idx,
        });
        used.insert(*idx);
    }

    Some(InlinePlan {
        teardown: build_teardown(source, &decl, defs),
        teardown_possible: teardown_possible(defs),
        teardown_clean: teardown_clean(defs, &decl, &used, model),
        eligible,
        total_reads: reaching.per_read().len(),
    })
}

fn duplicates_a_call_or_construct(eligible: &[EligibleRead]) -> bool {
    let mut per_def: HashMap<usize, usize> = HashMap::new();
    for read in eligible.iter().filter(|r| r.calls_or_constructs) {
        *per_def.entry(read.def_index).or_default() += 1;
    }
    per_def.values().any(|&count| count > 1)
}

fn teardown_possible(defs: &[ReachDef]) -> bool {
    defs.iter()
        .filter(|d| !d.is_decl())
        .all(|d| d.stmt().is_some())
}

fn teardown_clean(
    defs: &[ReachDef],
    decl: &Declaration,
    used: &HashSet<usize>,
    model: &BodyModel,
) -> bool {
    defs.iter().enumerate().all(|(i, def)| {
        // A used store's value moved into a read, so dropping its statement keeps the effect.
        if used.contains(&i) {
            return true;
        }
        let stmt = if def.is_decl() {
            Some(decl.stmt.clone())
        } else {
            def.stmt()
        };
        stmt.is_none_or(|s| !model.has_observable_effect(&s))
    })
}

fn build_teardown(source: &str, decl: &Declaration, defs: &[ReachDef]) -> Vec<Splice> {
    let mut teardown = vec![remove_binding(source, decl)];
    let mut seen = HashSet::from([decl.stmt.clone()]);
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

fn confidence(verified: bool) -> Confidence {
    if verified {
        Confidence::Verified
    } else {
        Confidence::Unverified
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
    let verified = plan.teardown_clean
        && plan.eligible.iter().all(|read| read.verified)
        && !duplicates_a_call_or_construct(&plan.eligible);
    Some(Inlining {
        plan: EditPlan {
            edits,
            confidence: confidence(verified),
        },
        scope,
    })
}

fn inline_single_read(range: &Range<usize>, plan: &InlinePlan) -> Option<Inlining> {
    let read = plan.eligible.iter().find(|read| read.range == *range)?;
    let mut edits = vec![Splice {
        range: range.clone(),
        text: read.text.clone(),
    }];
    let mut verified = read.verified;
    if plan.total_reads == 1 {
        if !plan.teardown_possible {
            return None;
        }
        edits.extend(plan.teardown.iter().cloned());
        verified = verified && plan.teardown_clean;
    } else if read.calls_or_constructs {
        // The declaration stays, so a call or construction would run there and at this read.
        verified = false;
    }
    Some(Inlining {
        plan: EditPlan {
            edits,
            confidence: confidence(verified),
        },
        scope: InlineScope::SingleUsage,
    })
}

fn remove_binding(source: &str, decl: &Declaration) -> Splice {
    if decl.names.len() == 1 {
        return delete_statement(source, decl.stmt.clone());
    }
    let i = decl.target_index;
    remove_list_entry(
        &decl.names[i],
        i.checked_sub(1).map(|p| &decl.names[p]),
        decl.names.get(i + 1),
    )
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
