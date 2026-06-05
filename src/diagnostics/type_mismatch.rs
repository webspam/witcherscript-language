use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::cst::grammar::call_callee;
use crate::cst::nav::first_named_child;
use crate::document::ParsedDocument;
use crate::resolve::{
    assignability, infer_type, resolve_definition_at_byte, Assignability, SymbolDb,
};
use crate::symbols::{node_text, Symbol, SymbolKind};
use crate::types::{native_type_accepts, Primitive, Type};

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct TypeMismatchRule;

impl CstRule for TypeMismatchRule {
    fn name(&self) -> &'static str {
        "type_mismatch"
    }

    fn interested_in(&self, kind: &str) -> bool {
        matches!(
            kind,
            "local_var_decl_stmt"
                | "assign_op_expr"
                | "func_call_expr"
                | "return_stmt"
                | "member_default_val"
                | "member_default_val_block_assign"
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
            "return_stmt" => check_return(node, ctx),
            "member_default_val" | "member_default_val_block_assign" => check_default(node, ctx),
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
            "type_mismatch",
            format!("Cannot assign value of type '{value_type}' to '{target}'"),
            Severity::Error,
            ctx,
        );
    }
}

fn check_assignment<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(op) = node.child_by_field_name("op") else {
        return;
    };
    let (Some(left), Some(right)) = (
        node.child_by_field_name("left"),
        node.child_by_field_name("right"),
    ) else {
        return;
    };
    let target = infer_type(ctx.uri, ctx.document, ctx.db, left, left.start_byte());
    // Compound-op result type is only modelled for primitive operands.
    if op.kind() != "assign_op_direct" && !matches!(target, Type::Primitive(_)) {
        return;
    }
    let value_type = infer_type(ctx.uri, ctx.document, ctx.db, right, right.start_byte());
    if is_incompatible(&value_type, &target, ctx.db) {
        emit(
            right,
            "type_mismatch",
            format!("Cannot assign value of type '{value_type}' to '{target}'"),
            Severity::Error,
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
    // Too many args is an arity error this rule doesn't own; bail before indexing past params.
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
                "type_mismatch",
                format!(
                    "Argument {} expects type '{target}' but got '{value_type}'",
                    i + 1
                ),
                Severity::Error,
                ctx,
            );
        }
    }
}

fn check_return<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let Some(value) = first_named_child(node) else {
        return;
    };
    let Some(callable) = ctx.document.symbols.enclosing_symbol_at(
        node.start_byte(),
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    ) else {
        return;
    };
    let Some(return_annot) = callable.type_annotation.as_deref() else {
        return;
    };
    let target = Type::from_annotation(return_annot);
    let value_type = infer_type(ctx.uri, ctx.document, ctx.db, value, value.start_byte());
    if is_incompatible(&value_type, &target, ctx.db) {
        emit(
            value,
            "type_mismatch",
            format!(
                "Cannot return value of type '{value_type}' from function returning '{target}'"
            ),
            Severity::Error,
            ctx,
        );
    }
}

fn check_default<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    let (Some(member), Some(value)) = (
        node.child_by_field_name("member"),
        node.child_by_field_name("value"),
    ) else {
        return;
    };
    let Some(def) = resolve_definition_at_byte(ctx.uri, ctx.document, ctx.db, member.start_byte())
    else {
        return;
    };
    if def.symbol.kind != SymbolKind::Field {
        return;
    }
    let Some(field_annot) = def.symbol.type_annotation.as_deref() else {
        return;
    };
    let target = Type::from_annotation(field_annot);
    // The compiler accepts a constant string literal as a `name`/`CName` default
    if value.kind() == "literal_string" && matches!(target, Type::Primitive(Primitive::Name)) {
        emit(
            value,
            "string_as_name_default",
            format!("String literal (double quotes) used for a '{target}' (single quotes) default"),
            Severity::Info,
            ctx,
        );
        return;
    }
    let value_type = infer_type(ctx.uri, ctx.document, ctx.db, value, value.start_byte());
    // The compiler accepts a float literal as an `int` default
    if matches!(value_type, Type::Primitive(Primitive::Float))
        && matches!(target, Type::Primitive(Primitive::Int))
    {
        emit(
            value,
            "float_as_int_default",
            format!("Float value used for an '{target}' default"),
            Severity::Info,
            ctx,
        );
        return;
    }
    if check_native_type_default(&target, value, &value_type, ctx) {
        return;
    }
    if is_incompatible(&value_type, &target, ctx.db) {
        emit(
            value,
            "type_mismatch",
            format!("Cannot assign value of type '{value_type}' to '{target}'"),
            Severity::Error,
            ctx,
        );
    }
}

/// A `CBehTreeVal*` native type accepts any primitive `default`; a non-exact one is coerced, flagged info-only, never an error.
fn check_native_type_default<'tree>(
    target: &Type,
    value: Node<'tree>,
    value_type: &Type,
    ctx: &mut CstRuleCtx<'_, 'tree>,
) -> bool {
    let Type::Named(name) = target else {
        return false;
    };
    let Some(accepted) = native_type_accepts(name) else {
        return false;
    };
    let exact = matches!(value_type, Type::Primitive(p) if accepted.contains(p));
    if !exact && !matches!(value_type, Type::Unknown) {
        emit(
            value,
            "native_default_coercion",
            format!("Value of type '{value_type}' coerced into native type '{target}' default"),
            Severity::Info,
            ctx,
        );
    }
    true
}

/// Argument slots of a call. `None` if no args or any slot is empty (`f(a,,b)`), which breaks positional alignment.
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
    if !def.symbol.kind.is_callable() {
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

/// Confidently judgeable: primitive, `NULL`, or a resolvable `Named`. Unresolved names (unindexed base type, generic `T`) skip to avoid false positives.
fn is_concrete(ty: &Type, db: &SymbolDb) -> bool {
    match ty {
        Type::Primitive(_) | Type::Null => true,
        Type::Array(elem) => is_concrete(elem, db),
        Type::Named(name) => db.find_top_level(name).is_some(),
        Type::Void | Type::Unknown => false,
    }
}

fn emit<'tree>(
    value_node: Node<'tree>,
    kind: &str,
    message: String,
    severity: Severity,
    ctx: &mut CstRuleCtx<'_, 'tree>,
) {
    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        value_node.start_byte(),
        value_node.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: kind.to_string(),
        message,
        severity,
        range,
        related: vec![],
        data: None,
    });
}

#[cfg(test)]
mod tests;
