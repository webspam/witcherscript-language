use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::grammar::member_access_member;
use crate::cst::kinds;
use crate::cst::nav::first_named_child;
use crate::strings::receiver_name;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};
use crate::types::Type;

use super::super::Definition;
use super::super::body_model::{BodyModel, LocalId};
use super::super::definition::definition_key;
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
    id: SymbolId,
    pub(super) local: LocalId,
    pub(super) name: String,
    pub(super) ty: Type,
    /// An `out` parameter of the enclosing callable: the caller observes every write.
    pub(super) always_live: bool,
}

pub(super) struct InternalLocal {
    id: SymbolId,
    pub(super) local: LocalId,
    name: String,
}

pub(super) fn collect_captures(
    ctx: &ResolveCtx,
    model: &BodyModel,
    roots: &[Node],
    range: &Range<usize>,
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
                            local: model.local_for(definition.symbol.id)?,
                            name: definition.symbol.name.clone(),
                        });
                    }
                    continue;
                }
                if !locals.iter().any(|l| l.id == definition.symbol.id) {
                    let ty = definition.symbol.type_annotation.clone()?;
                    if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
                        return None;
                    }
                    locals.push(CapturedLocal {
                        id: definition.symbol.id,
                        local: model.local_for(definition.symbol.id)?,
                        name: definition.symbol.name.clone(),
                        ty,
                        always_live: definition.symbol.kind == SymbolKind::Parameter
                            && definition.symbol.specifiers.is_out(),
                    });
                }
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
    detect_promoted_writes(model, range, &mut promoted);

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

fn detect_promoted_writes(model: &BodyModel, range: &Range<usize>, promoted: &mut [PromotedField]) {
    for field in promoted {
        field.is_written = model.field_written_in(&field.key, &field.ty, range);
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
