use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::grammar::{member_access_member, write_target};
use crate::cst::nav::first_named_child;
use crate::cst::{fields, kinds};
use crate::strings::receiver_name;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};
use crate::types::Type;

use super::super::Definition;
use super::super::definition::definition_key;
use super::super::extract_common::{WriteSite, write_sites};
use super::super::inference::TypeContext;
use super::super::symbol_db::SymbolDb;
use super::{Destination, ResolveCtx};

// Only valid inside the @wrapMethod body it belongs to; it cannot move into a global function.
const WRAPPED_METHOD_MACRO: &str = "wrappedMethod";

pub(super) struct Captures {
    pub(super) receiver: Option<Receiver>,
    pub(super) rewrites: Vec<BodyRewrite>,
    pub(super) locals: Vec<CapturedLocal>,
    pub(super) promoted: Vec<PromotedField>,
    pub(super) internals: Vec<InternalLocal>,
}

#[derive(Clone)]
pub(super) struct Receiver {
    pub(super) type_name: String,
    pub(super) param_name: String,
}

pub(super) enum BodyRewrite {
    /// Insert `<receiver>.` before a bare implicit-this public member.
    Qualify(usize),
    /// Replace a `this` expression (value, or receiver of a public member access) with the receiver.
    ReplaceThis(Range<usize>),
    /// Replace a private/protected field access (`this.f` or bare `f`) with its promoted parameter.
    ReplacePromoted { range: Range<usize>, field: usize },
}

// A global function only reaches public members, so a private/protected field reached through `this`
// is passed in by value (or `out`) instead of being squashed into an illegal `receiver.field`.
pub(super) struct PromotedField {
    key: (String, Range<usize>),
    pub(super) name: String,
    pub(super) ty: Type,
    pub(super) is_written: bool,
}

pub(super) struct CapturedLocal {
    pub(super) id: SymbolId,
    pub(super) name: String,
    pub(super) ty: Type,
    /// An `out` parameter of the enclosing callable: the caller observes every write.
    pub(super) always_live: bool,
    reads: Vec<usize>,
    writes: Vec<usize>,
    /// Statement ends of whole-value writes that run unconditionally within the selection.
    dominating_write_ends: Vec<usize>,
}

pub(super) struct InternalLocal {
    pub(super) id: SymbolId,
    name: String,
}

impl CapturedLocal {
    pub(super) fn is_written(&self) -> bool {
        !self.writes.is_empty()
    }

    // The entry value cannot reach a read once an unconditional whole-value write precedes them all.
    pub(super) fn entry_value_unread(&self) -> bool {
        match self.dominating_write_ends.iter().min() {
            Some(&kill) => self.reads.iter().all(|&read| read >= kill),
            None => false,
        }
    }
}

pub(super) fn collect_captures(
    ctx: &ResolveCtx,
    roots: &[Node],
    range: &Range<usize>,
    run_block: Option<Node>,
    callable: &Symbol,
    type_context: Option<&TypeContext>,
    destination: Destination,
) -> Option<Captures> {
    // super/parent name a relationship of the enclosing type; a global function cannot express
    // them, but a sibling method shares that relationship and keeps them verbatim.
    let unmovable = &[
        kinds::SUPER_EXPR,
        kinds::PARENT_EXPR,
        kinds::VIRTUAL_PARENT_EXPR,
    ];
    let mut references = Vec::new();
    for root in roots {
        if matches!(destination, Destination::GlobalFunction)
            && has_descendant_of_kind(*root, unmovable)
        {
            return None;
        }
        collect_descendants_of_kind(*root, &[kinds::IDENT, kinds::THIS_EXPR], &mut references);
    }
    references.sort_by_key(Node::start_byte);

    let source = ctx.document.source.as_bytes();
    let mut rewrites = Vec::new();
    let mut locals: Vec<CapturedLocal> = Vec::new();
    let mut promoted: Vec<PromotedField> = Vec::new();
    let mut internals: Vec<InternalLocal> = Vec::new();
    for reference in references {
        if reference.kind() == kinds::THIS_EXPR {
            // A sibling method shares `this`; only the global-function path reroutes it.
            if matches!(destination, Destination::Method) {
                continue;
            }
            match this_member(reference, ctx) {
                Some((member, def)) => match member_disposition(&def) {
                    Disposition::Receiver => {
                        rewrites.push(BodyRewrite::ReplaceThis(reference.byte_range()));
                    }
                    Disposition::Promote => {
                        let field = promote_field(&mut promoted, &def)?;
                        let range = reference.start_byte()..member.end_byte();
                        rewrites.push(BodyRewrite::ReplacePromoted { range, field });
                    }
                    Disposition::Refuse => return None,
                },
                None => rewrites.push(BodyRewrite::ReplaceThis(reference.byte_range())),
            }
            continue;
        }
        if is_member_slot(reference) {
            continue;
        }
        if reference.utf8_text(source).ok()? == WRAPPED_METHOD_MACRO {
            return None;
        }
        let Some(definition) = ctx.resolve_at(reference.start_byte()) else {
            continue;
        };
        match definition.symbol.kind {
            SymbolKind::Variable | SymbolKind::Parameter
                if definition.uri == ctx.uri
                    && definition.symbol.container == Some(callable.id) =>
            {
                if range.contains(&definition.symbol.selection_byte_range.start) {
                    if internals.iter().all(|i| i.id != definition.symbol.id) {
                        internals.push(InternalLocal {
                            id: definition.symbol.id,
                            name: definition.symbol.name.clone(),
                        });
                    }
                    continue;
                }
                let position = locals.iter().position(|l| l.id == definition.symbol.id);
                let index = if let Some(index) = position {
                    index
                } else {
                    let ty = definition.symbol.type_annotation.clone()?;
                    if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
                        return None;
                    }
                    locals.push(CapturedLocal {
                        id: definition.symbol.id,
                        name: definition.symbol.name.clone(),
                        ty,
                        always_live: definition.symbol.kind == SymbolKind::Parameter
                            && definition.symbol.specifiers.is_out(),
                        reads: Vec::new(),
                        writes: Vec::new(),
                        dominating_write_ends: Vec::new(),
                    });
                    locals.len() - 1
                };
                record_occurrence(&mut locals[index], reference, run_block);
            }
            SymbolKind::Field | SymbolKind::Method | SymbolKind::Event => {
                // A sibling method reaches every member of the enclosing type directly.
                if matches!(destination, Destination::Method) {
                    continue;
                }
                match member_disposition(&definition) {
                    Disposition::Receiver => {
                        rewrites.push(BodyRewrite::Qualify(reference.start_byte()));
                    }
                    Disposition::Promote => {
                        let field = promote_field(&mut promoted, &definition)?;
                        let range = reference.byte_range();
                        rewrites.push(BodyRewrite::ReplacePromoted { range, field });
                    }
                    Disposition::Refuse => return None,
                }
            }
            _ => {}
        }
    }
    record_indirect_writes(ctx, roots, &mut locals);

    let mut taken: HashSet<String> = locals
        .iter()
        .map(|l| l.name.clone())
        .chain(internals.iter().map(|i| i.name.clone()))
        .collect();
    for field in &mut promoted {
        let name = suffixed_unique(&field.name, |n| {
            taken.contains(n) || ctx.db.find_script_global(n).is_some()
        });
        taken.insert(name.clone());
        field.name = name;
    }
    detect_promoted_writes(ctx, roots, &mut promoted);

    let needs_receiver = rewrites
        .iter()
        .any(|r| matches!(r, BodyRewrite::Qualify(_) | BodyRewrite::ReplaceThis(_)));
    let receiver = if needs_receiver {
        Some(build_receiver(ctx.db, type_context?, &taken)?)
    } else {
        None
    };
    Some(Captures {
        receiver,
        rewrites,
        locals,
        promoted,
        internals,
    })
}

enum Disposition {
    Receiver,
    Promote,
    Refuse,
}

// A global function can only reach public members of the enclosing type.
fn member_disposition(definition: &Definition) -> Disposition {
    let public = definition.symbol.access == AccessLevel::Public;
    match definition.symbol.kind {
        SymbolKind::Field if !public => Disposition::Promote,
        SymbolKind::Method | SymbolKind::Event if !public => Disposition::Refuse,
        _ => Disposition::Receiver,
    }
}

fn this_member<'tree>(
    this_expr: Node<'tree>,
    ctx: &ResolveCtx,
) -> Option<(Node<'tree>, Definition)> {
    let parent = this_expr.parent()?;
    if parent.kind() != kinds::MEMBER_ACCESS_EXPR {
        return None;
    }
    if first_named_child(parent).map(|c| c.id()) != Some(this_expr.id()) {
        return None;
    }
    let member = member_access_member(parent)?;
    let definition = ctx.resolve_at(member.start_byte())?;
    Some((member, definition))
}

fn promote_field(promoted: &mut Vec<PromotedField>, definition: &Definition) -> Option<usize> {
    let key = definition_key(definition);
    if let Some(index) = promoted.iter().position(|p| p.key == key) {
        return Some(index);
    }
    let ty = definition.symbol.type_annotation.clone()?;
    if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
        return None;
    }
    promoted.push(PromotedField {
        key,
        name: definition.symbol.name.clone(),
        ty,
        is_written: false,
    });
    Some(promoted.len() - 1)
}

fn detect_promoted_writes(ctx: &ResolveCtx, roots: &[Node], promoted: &mut [PromotedField]) {
    if promoted.is_empty() {
        return;
    }
    for write in write_sites(ctx.uri, ctx.document, ctx.db, roots) {
        match write {
            WriteSite::AssignTarget(node) | WriteSite::OutArg(node) => {
                mark_field_written(ctx, node, promoted, false);
            }
            WriteSite::AssignBase(node) | WriteSite::ReceiverBase(node) => {
                mark_field_written(ctx, node, promoted, true);
            }
        }
    }
}

fn mark_field_written(
    ctx: &ResolveCtx,
    ident: Node,
    promoted: &mut [PromotedField],
    value_type_only: bool,
) {
    let Some(definition) = ctx.resolve_at(ident.start_byte()) else {
        return;
    };
    if definition.symbol.kind != SymbolKind::Field {
        return;
    }
    let key = definition_key(&definition);
    if let Some(field) = promoted.iter_mut().find(|p| p.key == key)
        && (!value_type_only || is_value_type(&field.ty, ctx.db))
    {
        field.is_written = true;
    }
}

fn is_member_slot(ident: Node) -> bool {
    ident.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            kinds::MEMBER_ACCESS_EXPR | kinds::INCOMPLETE_MEMBER_ACCESS_EXPR
        ) && member_access_member(parent).is_some_and(|member| member.id() == ident.id())
    })
}

fn record_occurrence(local: &mut CapturedLocal, ident: Node, run_block: Option<Node>) {
    let byte = ident.start_byte();
    match assignment_write(ident) {
        Some(AssignmentWrite::Whole(assign)) => {
            local.writes.push(byte);
            if let Some(end) = unconditional_statement_end(assign, run_block) {
                local.dominating_write_ends.push(end);
            }
        }
        Some(AssignmentWrite::Partial) => {
            local.reads.push(byte);
            local.writes.push(byte);
        }
        None => local.reads.push(byte),
    }
}

enum AssignmentWrite<'tree> {
    /// `x = ...`: replaces the whole value without reading it.
    Whole(Node<'tree>),
    /// Compound op or element/member path: the previous value flows into the result.
    Partial,
}

fn assignment_write(ident: Node) -> Option<AssignmentWrite> {
    let assign = find_ancestor_of_kind(ident, &[kinds::ASSIGN_OP_EXPR])?;
    let left = assign.child_by_field_name(fields::LEFT)?;
    if write_target(left).map(|n| n.id()) != Some(ident.id()) {
        return None;
    }
    let direct = assign
        .child_by_field_name(fields::OP)
        .is_some_and(|op| op.kind() == kinds::ASSIGN_OP_DIRECT);
    if direct && unwrap_nested(left).id() == ident.id() {
        Some(AssignmentWrite::Whole(assign))
    } else {
        Some(AssignmentWrite::Partial)
    }
}

fn unwrap_nested(expr: Node) -> Node {
    match expr.kind() {
        kinds::NESTED_EXPR => first_named_child(expr).map_or(expr, unwrap_nested),
        _ => expr,
    }
}

// Only a direct statement of the extracted run is guaranteed to execute; nested writes are conditional.
fn unconditional_statement_end(assign: Node, run_block: Option<Node>) -> Option<usize> {
    let block = run_block?;
    let stmt = assign.parent().filter(|p| p.kind() == kinds::EXPR_STMT)?;
    (stmt.parent()?.id() == block.id()).then(|| stmt.end_byte())
}

fn record_indirect_writes(ctx: &ResolveCtx, roots: &[Node], locals: &mut [CapturedLocal]) {
    for write in write_sites(ctx.uri, ctx.document, ctx.db, roots) {
        match write {
            // The direct assignment target's read/write is recorded from the reference itself.
            WriteSite::AssignTarget(_) => {}
            WriteSite::OutArg(node) => record_write(ctx, node, locals),
            // A path base or method receiver mutates a value type in place, not a shared handle.
            WriteSite::AssignBase(node) | WriteSite::ReceiverBase(node) => {
                record_value_type_write(ctx, node, locals);
            }
        }
    }
}

fn captured_local_mut<'a>(
    ctx: &ResolveCtx,
    ident: Node,
    locals: &'a mut [CapturedLocal],
) -> Option<&'a mut CapturedLocal> {
    let definition = ctx.resolve_at(ident.start_byte())?;
    if definition.uri != ctx.uri {
        return None;
    }
    locals.iter_mut().find(|l| l.id == definition.symbol.id)
}

// Indirect writes go through a reference, so the prior value counts as read too.
fn record_write(ctx: &ResolveCtx, ident: Node, locals: &mut [CapturedLocal]) {
    if let Some(local) = captured_local_mut(ctx, ident, locals) {
        local.reads.push(ident.start_byte());
        local.writes.push(ident.start_byte());
    }
}

fn record_value_type_write(ctx: &ResolveCtx, ident: Node, locals: &mut [CapturedLocal]) {
    let Some(definition) = ctx.resolve_at(ident.start_byte()) else {
        return;
    };
    if definition.uri != ctx.uri {
        return;
    }
    let Some(local) = locals.iter_mut().find(|l| l.id == definition.symbol.id) else {
        return;
    };
    if is_value_type(&local.ty, ctx.db) {
        local.reads.push(ident.start_byte());
        local.writes.push(ident.start_byte());
    }
}

// Arrays and structs copy on assignment and into parameters; classes are shared handles.
fn is_value_type(ty: &Type, db: &SymbolDb) -> bool {
    match ty {
        Type::Array(_) => true,
        Type::Named(name) => db
            .find_top_level(name)
            .is_some_and(|d| d.symbol.kind == SymbolKind::Struct),
        Type::Null | Type::Unknown | Type::Void | Type::Primitive(_) => false,
    }
}

fn build_receiver(
    db: &SymbolDb,
    type_context: &TypeContext,
    taken: &HashSet<String>,
) -> Option<Receiver> {
    // A state has no spellable parameter type; states wait for extract-to-method.
    if type_context.owner_class.is_some() {
        return None;
    }
    // A struct receiver param would be a copy, silently dropping member writes.
    if db
        .find_top_level(&type_context.name)
        .is_some_and(|d| d.symbol.kind == SymbolKind::Struct)
    {
        return None;
    }
    let param_name = suffixed_unique(&receiver_name(&type_context.name), |n| {
        taken.contains(n) || db.find_script_global(n).is_some()
    });
    Some(Receiver {
        type_name: type_context.name.clone(),
        param_name,
    })
}

pub(super) fn suffixed_unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    let mut suffix = 1usize;
    loop {
        let candidate = format!("{base}{suffix}");
        if !taken(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}
