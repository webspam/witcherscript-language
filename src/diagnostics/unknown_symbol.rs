use std::collections::HashMap;
use std::marker::PhantomData;
use std::time::Instant;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::{infer_expr_type_memo, resolve_definition_at_ident, SymbolDb, BUILTIN_TYPES};
use crate::symbols::{AccessLevel, SymbolKind};

use super::{
    access_is_inside_declaring_class, collect_nodes_with_error_subtree, declaring_class_of,
    run_parallel_pass, CstRuleCtx, ParallelRuleShard, Severity, WorkspaceDiagnostic,
};

pub(crate) fn run_unknown_symbol_parallel(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb<'_>,
) -> ParallelRuleShard {
    let items = collect_nodes_with_error_subtree(document.tree.root_node(), |k| k == "ident");
    let visits = items.len();
    let start = Instant::now();
    let shard = run_parallel_pass(
        &items,
        db,
        |node, in_err, local_db, memo, telemetry, diagnostics| {
            let mut ctx = CstRuleCtx {
                uri,
                document,
                db: local_db,
                type_memo: memo,
                telemetry,
                diagnostics,
                in_error_subtree: in_err,
                _tree: PhantomData,
            };
            check_ident(node, &mut ctx);
        },
    );
    let elapsed = start.elapsed();
    tracing::debug!(
        rule = "unknown_symbol",
        visits = visits,
        elapsed_us = elapsed.as_micros() as u64,
        "cst rule timing"
    );
    tracing::debug!(
        top_level = shard.telemetry.top_level_lookups,
        member = shard.telemetry.member_lookups,
        enum_variant = shard.telemetry.enum_variant_lookups,
        type_inference = shard.telemetry.type_inferences,
        definition = shard.telemetry.definition_resolutions,
        "cst lookup counts"
    );
    tracing::debug!(
        type_ref_us = shard.telemetry.branch_type_ref_us,
        type_ref_visits = shard.telemetry.branch_type_ref_visits,
        member_access_us = shard.telemetry.branch_member_access_us,
        member_access_visits = shard.telemetry.branch_member_access_visits,
        member_access_infer_us = shard.telemetry.member_access_infer_us,
        member_access_member_us = shard.telemetry.member_access_member_us,
        member_default_us = shard.telemetry.branch_member_default_us,
        member_default_visits = shard.telemetry.branch_member_default_visits,
        func_bare_call_us = shard.telemetry.branch_func_bare_call_us,
        func_bare_call_visits = shard.telemetry.branch_func_bare_call_visits,
        bare_us = shard.telemetry.branch_bare_us,
        bare_visits = shard.telemetry.branch_bare_visits,
        "unknown_symbol branch timing"
    );
    shard
}

pub fn collect_unknown_symbol_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let shard = run_unknown_symbol_parallel(uri, document, db);
        db.merge_observations(shard.observer);
        if !shard.diagnostics.is_empty() {
            debug!(
                uri = %uri,
                count = shard.diagnostics.len(),
                "emitted unknown-symbol diagnostics"
            );
            result.insert((*uri).to_string(), shard.diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for unknown symbols"
    );

    result
}

#[derive(Debug)]
enum IdentRole<'tree> {
    Declaration,
    TypeRef,
    MemberOfAccess(Node<'tree>),
    MemberOfDefaultOrHint { is_hint: bool },
    FuncBareCall,
    Bare,
}

fn check_ident<'tree>(ident: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    if ctx.in_error_subtree {
        return None;
    }

    let role = classify(ident)?;

    let name = ident.utf8_text(ctx.document.source.as_bytes()).ok()?;

    if name == "wrappedMethod" && is_inside_wrap_method(ident, ctx) {
        return None;
    }

    let branch_start = Instant::now();
    let result = match role {
        IdentRole::Declaration => None,
        IdentRole::TypeRef => {
            if BUILTIN_TYPES.contains(&name) {
                return None;
            }
            ctx.telemetry.definition_resolutions += 1;
            let r = if resolve_definition_at_ident(ctx.uri, ctx.document, ctx.db, ident).is_some() {
                None
            } else {
                push(ctx, ident, "unknown_type", format!("Unknown type '{name}'"));
                Some(())
            };
            ctx.telemetry.branch_type_ref_us += branch_start.elapsed().as_micros() as u64;
            ctx.telemetry.branch_type_ref_visits += 1;
            r
        }
        IdentRole::MemberOfAccess(receiver) => {
            ctx.telemetry.type_inferences += 1;
            let infer_start = Instant::now();
            let receiver_type = infer_expr_type_memo(
                ctx.uri,
                ctx.document,
                ctx.db,
                receiver,
                ident.start_byte(),
                ctx.type_memo,
            );
            ctx.telemetry.member_access_infer_us += infer_start.elapsed().as_micros() as u64;
            let r = (|| {
                let receiver_type = receiver_type?;
                ctx.telemetry.top_level_lookups += 1;
                let top = ctx.db.find_top_level(&receiver_type)?;
                if !matches!(
                    top.symbol.kind,
                    SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
                ) {
                    return None;
                }
                ctx.telemetry.member_lookups += 1;
                let member_start = Instant::now();
                let member = ctx
                    .db
                    .find_member(&receiver_type, name, AccessLevel::Private);
                ctx.telemetry.member_access_member_us += member_start.elapsed().as_micros() as u64;
                if let Some(def) = member {
                    if def.symbol.access == AccessLevel::Private
                        && !access_is_inside_declaring_class(ident, &def, ctx)
                    {
                        let declarer = declaring_class_of(&def).unwrap_or("");
                        push(
                            ctx,
                            ident,
                            "private_member_access",
                            format!("Private member '{name}' of class '{declarer}' is not accessible here."),
                        );
                        return Some(());
                    }
                    return None;
                }
                push(
                    ctx,
                    ident,
                    "unknown_member",
                    format!("No member '{name}' on type '{receiver_type}'"),
                );
                Some(())
            })();
            ctx.telemetry.branch_member_access_us += branch_start.elapsed().as_micros() as u64;
            ctx.telemetry.branch_member_access_visits += 1;
            r
        }
        IdentRole::MemberOfDefaultOrHint { is_hint } => {
            let r = (|| {
                let enclosing = ctx.document.symbols.enclosing_symbol_at(
                    ident.start_byte(),
                    &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
                )?;
                // `default autoState` sets a statemachine's initial state, not a declared member.
                if name == "autoState" && enclosing.is_state_machine {
                    return None;
                }
                let container_name = enclosing.name.clone();
                ctx.telemetry.member_lookups += 1;
                if ctx
                    .db
                    .find_member(&container_name, name, AccessLevel::Private)
                    .is_some()
                {
                    return None;
                }
                let severity = if is_hint {
                    Severity::Info
                } else {
                    Severity::Error
                };
                push_with_severity(
                    ctx,
                    ident,
                    "unknown_member",
                    format!("No member '{name}' on type '{container_name}'"),
                    severity,
                );
                Some(())
            })();
            ctx.telemetry.branch_member_default_us += branch_start.elapsed().as_micros() as u64;
            ctx.telemetry.branch_member_default_visits += 1;
            r
        }
        IdentRole::FuncBareCall => {
            ctx.telemetry.definition_resolutions += 1;
            let r = if resolve_definition_at_ident(ctx.uri, ctx.document, ctx.db, ident).is_some() {
                None
            } else {
                push(
                    ctx,
                    ident,
                    "unknown_function",
                    format!("Unknown function '{name}'"),
                );
                Some(())
            };
            ctx.telemetry.branch_func_bare_call_us += branch_start.elapsed().as_micros() as u64;
            ctx.telemetry.branch_func_bare_call_visits += 1;
            r
        }
        IdentRole::Bare => {
            let r = if resolves_as_local(ctx, ident, name) {
                None
            } else {
                ctx.telemetry.definition_resolutions += 1;
                if resolve_definition_at_ident(ctx.uri, ctx.document, ctx.db, ident).is_some() {
                    None
                } else {
                    push(
                        ctx,
                        ident,
                        "unknown_identifier",
                        format!("Unknown identifier '{name}'"),
                    );
                    Some(())
                }
            };
            ctx.telemetry.branch_bare_us += branch_start.elapsed().as_micros() as u64;
            ctx.telemetry.branch_bare_visits += 1;
            r
        }
    };
    result
}

fn classify(ident: Node<'_>) -> Option<IdentRole<'_>> {
    let parent = ident.parent()?;

    if is_declaration(ident, parent) {
        return Some(IdentRole::Declaration);
    }

    if is_type_reference(ident, parent) {
        return Some(IdentRole::TypeRef);
    }

    if let Some(kind) = crate::cst::grammar::ident_default_or_hint_kind(ident) {
        let is_hint = matches!(kind, crate::cst::grammar::DefaultOrHintKind::Hint);
        return Some(IdentRole::MemberOfDefaultOrHint { is_hint });
    }

    if parent.kind() == "member_access_expr" {
        let is_member = parent.child_by_field_name("member").map(|n| n.id()) == Some(ident.id());
        if is_member {
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "func_call_expr"
                    && grandparent.child_by_field_name("func").map(|n| n.id()) == Some(parent.id())
                {
                    return None;
                }
            }
            let receiver = parent.child_by_field_name("accessor")?;
            return Some(IdentRole::MemberOfAccess(receiver));
        }
    }

    if parent.kind() == "func_call_expr"
        && parent.child_by_field_name("func").map(|n| n.id()) == Some(ident.id())
    {
        return Some(IdentRole::FuncBareCall);
    }

    Some(IdentRole::Bare)
}

fn is_declaration(ident: Node, parent: Node) -> bool {
    match parent.kind() {
        "class_decl" | "struct_decl" | "enum_decl" | "state_decl" | "func_decl" | "event_decl"
        | "autobind_decl" | "enum_decl_variant" => {
            parent.child_by_field_name("name").map(|n| n.id()) == Some(ident.id())
        }
        "func_param_group" | "local_var_decl_stmt" | "member_var_decl" => {
            let mut cursor = parent.walk();
            let found = parent
                .children_by_field_name("names", &mut cursor)
                .any(|n| n.id() == ident.id());
            found
        }
        _ => false,
    }
}

fn is_type_reference(ident: Node, parent: Node) -> bool {
    match parent.kind() {
        "class_decl" | "state_decl" => {
            parent.child_by_field_name("base").map(|n| n.id()) == Some(ident.id())
                || parent.child_by_field_name("parent").map(|n| n.id()) == Some(ident.id())
        }
        "type_annot" => parent.child_by_field_name("type_name").map(|n| n.id()) == Some(ident.id()),
        "new_expr" => parent.child_by_field_name("class").map(|n| n.id()) == Some(ident.id()),
        "annotation" => parent.child_by_field_name("arg").map(|n| n.id()) == Some(ident.id()),
        "cast_expr" => {
            let mut cursor = parent.walk();
            let found = parent
                .children_by_field_name("type", &mut cursor)
                .any(|n| n.id() == ident.id());
            found
        }
        _ => false,
    }
}

fn resolves_as_local<'tree>(ctx: &CstRuleCtx<'_, 'tree>, ident: Node<'tree>, name: &str) -> bool {
    let byte = ident.start_byte();
    let Some(callable) = ctx.document.symbols.enclosing_symbol_at(
        byte,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    ) else {
        return false;
    };
    ctx.document
        .symbols
        .local_at_byte(callable.id, name, byte)
        .is_some()
}

fn is_inside_wrap_method<'tree>(ident: Node<'tree>, ctx: &CstRuleCtx<'_, 'tree>) -> bool {
    let Some(enclosing) = ctx.document.symbols.enclosing_symbol_at(
        ident.start_byte(),
        &[SymbolKind::Function, SymbolKind::Method],
    ) else {
        return false;
    };
    enclosing.annotations.iter().any(|a| a.name == "wrapMethod")
}

fn push<'tree>(ctx: &mut CstRuleCtx<'_, 'tree>, ident: Node<'tree>, kind: &str, message: String) {
    push_with_severity(ctx, ident, kind, message, Severity::Error);
}

fn push_with_severity<'tree>(
    ctx: &mut CstRuleCtx<'_, 'tree>,
    ident: Node<'tree>,
    kind: &str,
    message: String,
    severity: Severity,
) {
    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        ident.start_byte(),
        ident.end_byte(),
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
