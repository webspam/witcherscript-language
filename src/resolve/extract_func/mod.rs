mod captures;
mod render;
mod statements;

use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::{enclosing_callable_block, node_and_ancestors};
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::formatter::FormatOptions;
use crate::symbols::SymbolId;
use crate::types::Type;

use super::Definition;
use super::definition::resolve_definition_at_byte;
use super::extract_common::{
    CALLABLE_KINDS, Extraction, SelectionKind, Splice, applied_offset, classify_selection,
    is_call_callee, trim_selection,
};
use super::inference::{enclosing_type_context, infer_type};
use super::symbol_db::SymbolDb;

use captures::collect_captures;
use render::{
    FunctionPlan, assemble_params, call_expression, moved_text, render_function, statement_body,
    unique_function_name,
};
use statements::{has_escaping_control_flow, live_after, statement_run};

struct ResolveCtx<'a> {
    uri: &'a str,
    document: &'a ParsedDocument,
    db: &'a SymbolDb<'a>,
}

impl ResolveCtx<'_> {
    fn resolve_at(&self, byte: usize) -> Option<Definition> {
        resolve_definition_at_byte(self.uri, self.document, self.db, byte)
    }

    fn infer(&self, node: Node, byte: usize) -> Type {
        infer_type(self.uri, self.document, self.db, node, byte)
    }
}

pub fn extract_function(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    let selection = trim_selection(&document.source, selection)?;
    let root = document.tree.root_node();
    let ctx = ResolveCtx { uri, document, db };
    match classify_selection(root, &selection) {
        SelectionKind::Expression { node, range } => extract_expression(&ctx, node, range, options),
        SelectionKind::Statements { range } => extract_statements(&ctx, root, range, options),
    }
}

fn extract_expression(
    ctx: &ResolveCtx,
    node: Node,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    if is_call_callee(node) {
        // A bare reference to the callee is a function reference, which WitcherScript has no values for.
        return None;
    }
    enclosing_callable_block(node)?;
    let callable = ctx
        .document
        .symbols
        .enclosing_symbol_at(selection.start, CALLABLE_KINDS)?;
    let ty = ctx.infer(node, selection.start);
    if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
        return None;
    }
    let type_context = enclosing_type_context(ctx.document, ctx.db, selection.start);
    let captures = collect_captures(
        ctx,
        &[node],
        &selection,
        None,
        callable,
        type_context.as_ref(),
    )?;
    let body = format!(
        "return {};",
        moved_text(&ctx.document.source, &selection, &captures)
    );
    let (value_locals, out_locals): (Vec<_>, Vec<_>) =
        captures.locals.iter().partition(|l| !l.is_written());
    let plan = FunctionPlan {
        name: unique_function_name(ctx.document, ctx.db, callable, type_context.as_ref()),
        receiver: captures.receiver.clone(),
        params: assemble_params(&value_locals, &out_locals, &captures.promoted),
        return_type: ty,
        body,
    };
    let call_text = call_expression(&plan);
    build_extraction(ctx.document, node, selection, call_text, 0, &plan, options)
}

fn extract_statements(
    ctx: &ResolveCtx,
    root: Node,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    let source = &ctx.document.source;
    let (run_block, stmts, range) = statement_run(root, source, &selection)?;
    if has_escaping_control_flow(&stmts, &range) {
        return None;
    }
    let first = *stmts.first()?;
    let callable_block = enclosing_callable_block(first)?;
    let callable = ctx
        .document
        .symbols
        .enclosing_symbol_at(range.start, CALLABLE_KINDS)?;
    let type_context = enclosing_type_context(ctx.document, ctx.db, range.start);
    let captures = collect_captures(
        ctx,
        &stmts,
        &range,
        Some(run_block),
        callable,
        type_context.as_ref(),
    )?;

    let mut tracked: HashSet<SymbolId> = captures
        .locals
        .iter()
        .filter(|l| l.is_written())
        .map(|l| l.id)
        .collect();
    tracked.extend(captures.internals.iter().map(|i| i.id));
    let live = live_after(ctx, callable_block, first, &range, &tracked);
    if captures.internals.iter().any(|i| live.contains(&i.id)) {
        // A local declared in the selection but used after it cannot move wholesale.
        return None;
    }

    let outputs: Vec<usize> = captures
        .locals
        .iter()
        .enumerate()
        .filter(|(_, l)| l.is_written() && (l.always_live || live.contains(&l.id)))
        .map(|(i, _)| i)
        .collect();
    let returned = match outputs.as_slice() {
        [only] if captures.locals[*only].entry_value_unread() => Some(*only),
        _ => None,
    };
    let mut value_locals = Vec::new();
    let mut out_locals = Vec::new();
    for (i, local) in captures.locals.iter().enumerate() {
        if returned == Some(i) {
            continue;
        }
        if outputs.contains(&i) {
            out_locals.push(local);
        } else {
            value_locals.push(local);
        }
    }
    let returned = returned.map(|i| &captures.locals[i]);

    let plan = FunctionPlan {
        name: unique_function_name(ctx.document, ctx.db, callable, type_context.as_ref()),
        receiver: captures.receiver.clone(),
        params: assemble_params(&value_locals, &out_locals, &captures.promoted),
        return_type: returned.map_or(Type::Void, |r| r.ty.clone()),
        body: statement_body(source, &range, &captures, returned, options),
    };
    let call = call_expression(&plan);
    let (call_text, cursor_prefix) = match returned {
        Some(r) => (format!("{} = {call};", r.name), r.name.len() + " = ".len()),
        None => (format!("{call};"), 0),
    };
    build_extraction(
        ctx.document,
        first,
        range,
        call_text,
        cursor_prefix,
        &plan,
        options,
    )
}

fn build_extraction(
    document: &ParsedDocument,
    inside_top_level: Node,
    replace: Range<usize>,
    call_text: String,
    cursor_prefix: usize,
    plan: &FunctionPlan,
    options: FormatOptions,
) -> Option<Extraction> {
    let function_text = render_function(plan, options);
    let insert_at = enclosing_top_level(inside_top_level)?.end_byte();
    // A following declaration needs a blank line after the inserted function too.
    let trailing = if document.source[insert_at..].trim().is_empty() {
        ""
    } else {
        "\n"
    };
    let anchor = replace.start;
    let edits = vec![
        Splice {
            range: insert_at..insert_at,
            text: format!("\n\n{function_text}{trailing}"),
        },
        Splice {
            range: replace,
            text: call_text,
        },
    ];
    let cursor = applied_offset(&edits, anchor) + cursor_prefix;
    Some(Extraction {
        edits,
        name: plan.name.clone(),
        cursor,
    })
}

fn enclosing_top_level(node: Node) -> Option<Node> {
    node_and_ancestors(node)
        .take_while(|n| n.kind() != kinds::SCRIPT)
        .last()
}
