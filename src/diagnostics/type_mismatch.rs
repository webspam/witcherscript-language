use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::grammar::call_callee;
use crate::document::ParsedDocument;
use crate::resolve::{
    assignability, infer_type, resolve_definition_at_byte, Assignability, SymbolDb,
};
use crate::symbols::{node_text, Symbol, SymbolKind};
use crate::types::Type;

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct TypeMismatchRule;

impl CstRule for TypeMismatchRule {
    fn name(&self) -> &'static str {
        "type_mismatch"
    }

    fn interested_in(&self, kind: &str) -> bool {
        matches!(
            kind,
            "local_var_decl_stmt" | "assign_op_expr" | "func_call_expr"
        )
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        match node.kind() {
            "local_var_decl_stmt" => check_var_decl(node, ctx),
            "assign_op_expr" => check_assignment(node, ctx),
            "func_call_expr" => check_call_args(node, ctx),
            _ => {}
        }
    }
}

pub fn collect_type_mismatch_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = TypeMismatchRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(uri = %uri, count = diagnostics.len(), "emitted type-mismatch diagnostics");
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for type mismatches"
    );
    result
}

fn check_var_decl<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(value) = node.child_by_field_name("init_value") else {
        return;
    };
    let Some(var_type) = node.child_by_field_name("var_type") else {
        return;
    };
    let target = Type::from_annotation(&node_text(var_type, &ctx.document.source));
    let value_type = infer_type(ctx.uri, ctx.document, ctx.db, value, value.start_byte());
    if is_incompatible(&value_type, &target, ctx.db) {
        emit(
            value,
            format!("Cannot assign value of type '{value_type}' to '{target}'"),
            ctx,
        );
    }
}

fn check_assignment<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(op) = node.child_by_field_name("op") else {
        return;
    };
    if op.kind() != "assign_op_direct" {
        return;
    }
    let (Some(left), Some(right)) = (
        node.child_by_field_name("left"),
        node.child_by_field_name("right"),
    ) else {
        return;
    };
    let target = infer_type(ctx.uri, ctx.document, ctx.db, left, left.start_byte());
    let value_type = infer_type(ctx.uri, ctx.document, ctx.db, right, right.start_byte());
    if is_incompatible(&value_type, &target, ctx.db) {
        emit(
            right,
            format!("Cannot assign value of type '{value_type}' to '{target}'"),
            ctx,
        );
    }
}

fn check_call_args<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(slots) = arg_slots(node) else {
        return;
    };
    let Some(params) = callee_params(node, ctx) else {
        return;
    };
    // More args than declared parameters means an overload/vararg we don't model.
    if slots.len() > params.len() {
        return;
    }
    for (i, arg) in slots.iter().enumerate() {
        let Some(target_annot) = params[i].type_annotation.as_deref() else {
            continue;
        };
        let target = Type::from_annotation(target_annot);
        let value_type = infer_type(ctx.uri, ctx.document, ctx.db, *arg, arg.start_byte());
        if is_incompatible(&value_type, &target, ctx.db) {
            emit(
                *arg,
                format!(
                    "Argument {} expects type '{target}' but got '{value_type}'",
                    i + 1
                ),
                ctx,
            );
        }
    }
}

/// Comma-delimited argument slots of a `func_call_expr`. Returns `None` when
/// the call has no arguments or any slot is empty (`f(a,,b)`) - an empty slot
/// breaks positional alignment, so the whole call is skipped.
fn arg_slots<'tree>(call: Node<'tree>) -> Option<Vec<Node<'tree>>> {
    let args = call.child_by_field_name("args")?;
    let mut slots: Vec<Option<Node>> = Vec::new();
    let mut pending: Option<Node> = None;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        match child.kind() {
            "," => slots.push(pending.take()),
            "comment" => {}
            _ if child.is_named() => pending = Some(child),
            _ => {}
        }
    }
    slots.push(pending.take());
    slots.into_iter().collect()
}

fn callee_params(call: Node, ctx: &mut CstRuleCtx) -> Option<Vec<Symbol>> {
    let callee = call_callee(call)?;
    let callee_ident = match callee.kind() {
        "ident" => callee,
        "member_access_expr" => callee
            .child_by_field_name("member")
            .filter(|m| m.kind() == "ident")?,
        _ => return None,
    };
    let def = resolve_definition_at_byte(ctx.uri, ctx.document, ctx.db, callee_ident.start_byte())?;
    if !matches!(
        def.symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Event
    ) {
        return None;
    }
    Some(ctx.db.full_parameters_of(&def.uri, def.symbol.id))
}

fn is_incompatible(value_type: &Type, target: &Type, db: &SymbolDb) -> bool {
    if !is_concrete(value_type, db) || !is_concrete(target, db) {
        return false;
    }
    assignability(value_type, target, db) == Assignability::Incompatible
}

/// A type we can judge with confidence: a primitive, `NULL`, or a `Named` type
/// that actually resolves. An unresolved name (a base type we failed to index,
/// or the unsubstituted generic placeholder `T`) is not concrete, so we skip
/// it rather than risk a false positive.
fn is_concrete(ty: &Type, db: &SymbolDb) -> bool {
    match ty {
        Type::Primitive(_) | Type::Null => true,
        Type::Array(elem) => is_concrete(elem, db),
        Type::Named(name) => db.find_top_level(name).is_some(),
        Type::Void | Type::Unknown => false,
    }
}

fn emit<'tree>(value_node: Node<'tree>, message: String, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        value_node.start_byte(),
        value_node.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: "type_mismatch".to_string(),
        message,
        severity: Severity::Error,
        range,
        related: vec![],
        data: None,
    });
}

#[cfg(test)]
mod tests;
