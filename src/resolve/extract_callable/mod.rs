mod captures;
mod render;
mod statements;

use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::{enclosing_callable_block, find_ancestor_of_kind, node_and_ancestors};
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::formatter::{FormatOptions, indent_block};
use crate::symbols::{Symbol, SymbolKind};
use crate::types::Type;

use super::Definition;
use super::body_model::BodyModel;
use super::definition::resolve_definition_at_byte;
use super::extract_common::{CALLABLE_KINDS, Extraction, insert_and_replace};
use super::inference::{enclosing_type_context, infer_type};
use super::selection::{SelectionKind, classify_selection, is_call_callee, trim_selection};
use super::symbol_db::SymbolDb;

use captures::collect_captures;
use render::{
    FunctionPlan, assemble_params, call_expression, moved_text, render_function, statement_body,
    unique_function_name,
};
use statements::{has_escaping_control_flow, statement_run};

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

// Where an extracted callable lands: this fixes what it can reach and how it is rendered.
#[derive(Clone, Copy)]
enum Destination {
    /// A new top-level free function (extract-to-function).
    GlobalFunction,
    /// A new sibling method of the enclosing class or state (extract-to-method).
    Method,
}

pub fn extract_function(
    model: &BodyModel,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    extract(model, selection, options, Destination::GlobalFunction)
}

pub fn extract_method(
    model: &BodyModel,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    extract(model, selection, options, Destination::Method)
}

fn extract(
    model: &BodyModel,
    selection: Range<usize>,
    options: FormatOptions,
    destination: Destination,
) -> Option<Extraction> {
    let document = model.document();
    let selection = trim_selection(&document.source, selection)?;
    let root = document.tree.root_node();
    let ctx = ResolveCtx {
        uri: model.uri(),
        document,
        db: model.db(),
    };
    match classify_selection(root, &selection) {
        SelectionKind::Expression { node, range } => {
            extract_expression(&ctx, model, node, range, options, destination)
        }
        SelectionKind::Statements { range } => {
            extract_statements(&ctx, model, root, range, options, destination)
        }
    }
}

// A method needs an enclosing class or state to hold it; a free function offers only itself.
fn method_host(document: &ParsedDocument, callable: &Symbol) -> Option<()> {
    callable
        .container
        .and_then(|id| document.symbols.by_id(id))
        .filter(|host| matches!(host.kind, SymbolKind::Class | SymbolKind::State))
        .map(|_| ())
}

fn modifier_for(destination: Destination) -> Option<&'static str> {
    match destination {
        Destination::Method => Some("private"),
        Destination::GlobalFunction => None,
    }
}

fn default_name(destination: Destination) -> &'static str {
    match destination {
        Destination::Method => "NewMethod",
        Destination::GlobalFunction => "NewFunction",
    }
}

fn extract_expression(
    ctx: &ResolveCtx,
    model: &BodyModel,
    node: Node,
    selection: Range<usize>,
    options: FormatOptions,
    destination: Destination,
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
    if matches!(destination, Destination::Method) {
        method_host(ctx.document, callable)?;
    }
    let ty = ctx.infer(node, selection.start);
    if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
        return None;
    }
    let type_context = enclosing_type_context(ctx.document, ctx.db, selection.start);
    let captures = collect_captures(
        ctx,
        model,
        &[node],
        &selection,
        callable,
        type_context.as_ref(),
        destination,
    )?;
    let body = format!(
        "return {};",
        moved_text(&ctx.document.source, &selection, &captures)
    );
    let (value_locals, out_locals): (Vec<_>, Vec<_>) = captures
        .locals
        .iter()
        .partition(|l| !model.is_written_in(l.local, &selection));
    let plan = FunctionPlan {
        name: unique_function_name(
            ctx.document,
            ctx.db,
            callable,
            type_context.as_ref(),
            default_name(destination),
        ),
        modifier: modifier_for(destination),
        receiver: captures.receiver.clone(),
        params: assemble_params(&value_locals, &out_locals, &captures.promoted),
        return_type: ty,
        body,
    };
    let call_text = call_expression(&plan);
    let (insert_at, insert_text) = placement(ctx.document, node, &plan, options, destination)?;
    Some(insert_and_replace(
        insert_at,
        insert_text,
        selection,
        call_text,
        0,
        plan.name,
    ))
}

fn extract_statements(
    ctx: &ResolveCtx,
    model: &BodyModel,
    root: Node,
    selection: Range<usize>,
    options: FormatOptions,
    destination: Destination,
) -> Option<Extraction> {
    let source = &ctx.document.source;
    let (run_block, stmts, range) = statement_run(root, source, &selection)?;
    if has_escaping_control_flow(&stmts, &range) {
        return None;
    }
    let first = *stmts.first()?;
    let callable = ctx
        .document
        .symbols
        .enclosing_symbol_at(range.start, CALLABLE_KINDS)?;
    if matches!(destination, Destination::Method) {
        method_host(ctx.document, callable)?;
    }
    let type_context = enclosing_type_context(ctx.document, ctx.db, range.start);
    let captures = collect_captures(
        ctx,
        model,
        &stmts,
        &range,
        callable,
        type_context.as_ref(),
        destination,
    )?;

    if captures
        .internals
        .iter()
        .any(|i| model.live_after(i.local, &range))
    {
        // A local declared in the selection but used after it cannot move wholesale.
        return None;
    }

    let outputs: Vec<usize> = captures
        .locals
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            model.is_written_in(l.local, &range)
                && (l.always_live || model.live_after(l.local, &range))
        })
        .map(|(i, _)| i)
        .collect();
    let run_block_range = run_block.byte_range();
    let returned = match outputs.as_slice() {
        [only]
            if model.entry_value_unread_in(
                captures.locals[*only].local,
                &range,
                &run_block_range,
            ) =>
        {
            Some(*only)
        }
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
        name: unique_function_name(
            ctx.document,
            ctx.db,
            callable,
            type_context.as_ref(),
            default_name(destination),
        ),
        modifier: modifier_for(destination),
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
    let (insert_at, insert_text) = placement(ctx.document, first, &plan, options, destination)?;
    Some(insert_and_replace(
        insert_at,
        insert_text,
        range,
        call_text,
        cursor_prefix,
        plan.name,
    ))
}

fn placement(
    document: &ParsedDocument,
    anchor: Node,
    plan: &FunctionPlan,
    options: FormatOptions,
    destination: Destination,
) -> Option<(usize, String)> {
    match destination {
        Destination::GlobalFunction => global_insertion(document, anchor, plan, options),
        Destination::Method => method_insertion(document, anchor, plan, options),
    }
}

fn global_insertion(
    document: &ParsedDocument,
    anchor: Node,
    plan: &FunctionPlan,
    options: FormatOptions,
) -> Option<(usize, String)> {
    let function_text = render_function(plan, options);
    let insert_at = enclosing_top_level(anchor)?.end_byte();
    // A following declaration needs a blank line after the inserted function too.
    let trailing = if document.source[insert_at..].trim().is_empty() {
        ""
    } else {
        "\n"
    };
    Some((insert_at, format!("\n\n{function_text}{trailing}")))
}

fn method_insertion(
    document: &ParsedDocument,
    anchor: Node,
    plan: &FunctionPlan,
    options: FormatOptions,
) -> Option<(usize, String)> {
    let method_decl = find_ancestor_of_kind(anchor, &[kinds::FUNC_DECL, kinds::EVENT_DECL])?;
    let method_text = indent_block(&render_function(plan, options), &options);
    let insert_at = method_decl.end_byte();
    // A following member wants a blank line after the method, but the type's closing brace does not.
    let rest = document.source[insert_at..].trim_start();
    let trailing = if rest.starts_with('}') || rest.is_empty() {
        ""
    } else {
        "\n"
    };
    Some((insert_at, format!("\n\n{method_text}{trailing}")))
}

fn enclosing_top_level(node: Node) -> Option<Node> {
    node_and_ancestors(node)
        .take_while(|n| n.kind() != kinds::SCRIPT)
        .last()
}
