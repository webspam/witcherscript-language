use tree_sitter::Node;

use crate::cst::descendants::collect_descendants_of_kind;
use crate::cst::grammar::{arg_slots, call_callee, member_access_member, write_target};
use crate::cst::nav::first_named_child;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::SymbolKind;
use crate::types::Type;

use super::definition::callee_params;
use super::symbol_db::SymbolDb;

pub(super) enum WriteSite<'tree> {
    /// `x = ...`: the assignment's left-hand lvalue.
    AssignTarget(Node<'tree>),
    /// `x.f = ...` / `a[i] = ...`: the base local mutated in place (distinct from the target).
    AssignBase(Node<'tree>),
    /// `f(out x)`: an argument bound to an `out` parameter.
    OutArg(Node<'tree>),
    /// `x.Method()`: the receiver base, mutated in place when it is a value type.
    ReceiverBase(Node<'tree>),
}

pub(super) fn write_sites<'tree>(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    roots: &[Node<'tree>],
) -> Vec<WriteSite<'tree>> {
    let mut nodes = Vec::new();
    for root in roots {
        collect_descendants_of_kind(
            *root,
            &[kinds::ASSIGN_OP_EXPR, kinds::FUNC_CALL_EXPR],
            &mut nodes,
        );
    }
    let mut writes = Vec::new();
    for site in nodes {
        if site.kind() == kinds::ASSIGN_OP_EXPR {
            let Some(left) = site.child_by_field_name(fields::LEFT) else {
                continue;
            };
            if let Some(target) = write_target(left) {
                writes.push(WriteSite::AssignTarget(target));
                // `pos.x = 1` also mutates the base value; a bare `x = 1` is the target itself.
                if let Some(base) = lvalue_base_ident(left)
                    && base.id() != target.id()
                {
                    writes.push(WriteSite::AssignBase(base));
                }
            }
        } else {
            for arg in out_args(uri, document, db, site) {
                if let Some(target) = write_target(arg) {
                    writes.push(WriteSite::OutArg(target));
                }
            }
            if let Some(base) = method_call_receiver_base(site) {
                writes.push(WriteSite::ReceiverBase(base));
            }
        }
    }
    writes
}

pub(super) fn write_site_node<'tree>(site: &WriteSite<'tree>) -> Node<'tree> {
    match site {
        WriteSite::AssignTarget(n)
        | WriteSite::AssignBase(n)
        | WriteSite::OutArg(n)
        | WriteSite::ReceiverBase(n) => *n,
    }
}

fn lvalue_base_ident(expr: Node) -> Option<Node> {
    match expr.kind() {
        kinds::IDENT => Some(expr),
        kinds::MEMBER_ACCESS_EXPR => {
            let child = first_named_child(expr)?;
            // `this.field` is rooted at the field, not at an outer local.
            if child.kind() == kinds::THIS_EXPR {
                member_access_member(expr)
            } else {
                lvalue_base_ident(child)
            }
        }
        kinds::NESTED_EXPR => lvalue_base_ident(first_named_child(expr)?),
        kinds::ARRAY_EXPR => lvalue_base_ident(expr.child_by_field_name(fields::ACCESSOR)?),
        _ => None,
    }
}

fn method_call_receiver_base(call: Node) -> Option<Node> {
    let callee = call_callee(call)?;
    if callee.kind() != kinds::MEMBER_ACCESS_EXPR {
        return None;
    }
    lvalue_base_ident(first_named_child(callee)?)
}

// Arrays and structs copy on assignment and into parameters; classes are shared handles.
pub(super) fn is_value_type(ty: &Type, db: &SymbolDb) -> bool {
    match ty {
        Type::Array(_) => true,
        Type::Named(name) => db
            .find_top_level(name)
            .is_some_and(|d| d.symbol.kind == SymbolKind::Struct),
        Type::Null | Type::Unknown | Type::Void | Type::Primitive(_) => false,
    }
}

fn out_args<'tree>(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    call: Node<'tree>,
) -> Vec<Node<'tree>> {
    let Some(slots) = arg_slots(call) else {
        return Vec::new();
    };
    let Some(params) = callee_params(uri, document, db, call) else {
        return Vec::new();
    };
    params
        .iter()
        .zip(slots)
        .filter(|(parameter, _)| parameter.specifiers.is_out())
        .map(|(_, arg)| arg)
        .collect()
}
