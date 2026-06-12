use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::{enclosing_callable_block, find_ancestor_of_kind, node_and_ancestors};
use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::grammar::{call_callee, member_access_member, write_target};
use crate::cst::nav::first_named_child;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::formatter::{FormatOptions, indent_block, line_indent};
use crate::strings::receiver_name;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};
use crate::types::Type;

use super::definition::resolve_definition_at_byte;
use super::extract_var::{
    CALLABLE_KINDS, Extraction, Splice, applied_offset, apply_splices, exact_expression_at,
    expand_selection, is_call_callee, out_args, trim_selection,
};
use super::inference::{TypeContext, enclosing_type_context, infer_type};
use super::symbol_db::SymbolDb;

const DEFAULT_FUNCTION_NAME: &str = "NewFunction";

// Only valid inside the @wrapMethod body it belongs to; it cannot move into a global function.
const WRAPPED_METHOD_MACRO: &str = "wrappedMethod";

const LOOP_KINDS: &[&str] = &[kinds::FOR_STMT, kinds::WHILE_STMT, kinds::DO_WHILE_STMT];

pub fn extract_function(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    let selection = trim_selection(&document.source, selection)?;
    let root = document.tree.root_node();
    let expanded = expand_selection(root, &selection).unwrap_or_else(|| selection.clone());
    match exact_expression_at(root, &expanded) {
        Some(node) => extract_expression(uri, document, db, node, expanded, options),
        None => extract_statements(uri, document, db, root, selection, options),
    }
}

fn extract_expression(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    if is_call_callee(node) {
        // A bare reference to the callee is a function reference, which WitcherScript has no values for.
        return None;
    }
    enclosing_callable_block(node)?;
    let callable = document
        .symbols
        .enclosing_symbol_at(selection.start, CALLABLE_KINDS)?;
    let ty = infer_type(uri, document, db, node, selection.start);
    if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
        return None;
    }
    let type_context = enclosing_type_context(document, db, selection.start);
    let captures = collect_captures(
        uri,
        document,
        db,
        &[node],
        &selection,
        None,
        callable,
        type_context.as_ref(),
    )?;
    let body = format!(
        "return {};",
        moved_text(&document.source, &selection, &captures)
    );
    let (value_params, out_params): (Vec<_>, Vec<_>) =
        captures.locals.iter().partition(|l| !l.is_written());
    let plan = FunctionPlan {
        name: unique_function_name(document, db, callable, type_context.as_ref()),
        receiver: captures.receiver.as_ref(),
        value_params,
        out_params,
        return_type: ty,
        body,
    };
    let call_text = call_expression(&plan);
    build_extraction(document, node, selection, call_text, 0, &plan, options)
}

fn extract_statements(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    root: Node,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    let source = &document.source;
    let (run_block, stmts, range) = statement_run(root, source, &selection)?;
    if has_escaping_control_flow(&stmts, &range) {
        return None;
    }
    let first = *stmts.first()?;
    let callable_block = enclosing_callable_block(first)?;
    let callable = document
        .symbols
        .enclosing_symbol_at(range.start, CALLABLE_KINDS)?;
    let type_context = enclosing_type_context(document, db, range.start);
    let captures = collect_captures(
        uri,
        document,
        db,
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
    let live = live_after(uri, document, db, callable_block, first, &range, &tracked);
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
    let mut value_params = Vec::new();
    let mut out_params = Vec::new();
    for (i, local) in captures.locals.iter().enumerate() {
        if returned == Some(i) {
            continue;
        }
        if outputs.contains(&i) {
            out_params.push(local);
        } else {
            value_params.push(local);
        }
    }
    let returned = returned.map(|i| &captures.locals[i]);

    let plan = FunctionPlan {
        name: unique_function_name(document, db, callable, type_context.as_ref()),
        receiver: captures.receiver.as_ref(),
        value_params,
        out_params,
        return_type: returned.map_or(Type::Void, |r| r.ty.clone()),
        body: statement_body(source, &range, &captures, returned, options),
    };
    let call = call_expression(&plan);
    let (call_text, cursor_prefix) = match returned {
        Some(r) => (format!("{} = {call};", r.name), r.name.len() + " = ".len()),
        None => (format!("{call};"), 0),
    };
    build_extraction(
        document,
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

fn statement_run<'tree>(
    root: Node<'tree>,
    source: &str,
    selection: &Range<usize>,
) -> Option<(Node<'tree>, Vec<Node<'tree>>, Range<usize>)> {
    let probe = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    let block = node_and_ancestors(probe).find(|n| n.kind() == kinds::FUNC_BLOCK)?;
    let mut stmts = Vec::new();
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        let inside = selection.start <= child.start_byte() && child.end_byte() <= selection.end;
        let overlaps = child.start_byte() < selection.end && selection.start < child.end_byte();
        if inside {
            stmts.push(child);
        } else if overlaps {
            // A partially covered statement is ambiguous; expression mode handles sub-statement picks.
            return None;
        }
    }
    if stmts
        .iter()
        .all(|s| matches!(s.kind(), kinds::NOP | kinds::COMMENT))
    {
        return None;
    }
    let snapped = stmts.first()?.start_byte()..stmts.last()?.end_byte();
    let leading = &source[selection.start..snapped.start];
    let trailing = &source[snapped.end..selection.end];
    if !leading.trim().is_empty() || !trailing.trim().is_empty() {
        return None;
    }
    Some((block, stmts, snapped))
}

fn has_escaping_control_flow(stmts: &[Node], range: &Range<usize>) -> bool {
    let mut jumps = Vec::new();
    for stmt in stmts {
        if has_descendant_of_kind(*stmt, &[kinds::RETURN_STMT]) {
            return true;
        }
        collect_descendants_of_kind(
            *stmt,
            &[kinds::BREAK_STMT, kinds::CONTINUE_STMT],
            &mut jumps,
        );
    }
    jumps.iter().any(|jump| !jump_target_inside(*jump, range))
}

fn jump_target_inside(jump: Node, range: &Range<usize>) -> bool {
    node_and_ancestors(jump)
        .take_while(|n| range.start <= n.start_byte() && n.end_byte() <= range.end)
        .any(|n| {
            LOOP_KINDS.contains(&n.kind())
                || (jump.kind() == kinds::BREAK_STMT && n.kind() == kinds::SWITCH_STMT)
        })
}

fn live_after(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    callable_block: Node,
    first_stmt: Node,
    range: &Range<usize>,
    tracked: &HashSet<SymbolId>,
) -> HashSet<SymbolId> {
    if tracked.is_empty() {
        return HashSet::new();
    }
    let mut windows = Vec::with_capacity(2);
    windows.push(range.end..callable_block.end_byte());
    // A loop around the selection re-runs code that sits textually before it.
    if let Some(loop_node) = enclosing_loop(first_stmt, callable_block) {
        windows.push(loop_node.start_byte()..range.start);
    }
    let mut idents = Vec::new();
    collect_descendants_of_kind(callable_block, &[kinds::IDENT], &mut idents);
    idents
        .iter()
        .filter(|ident| windows.iter().any(|w| w.contains(&ident.start_byte())))
        .filter_map(|ident| resolve_definition_at_byte(uri, document, db, ident.start_byte()))
        .filter(|def| def.uri == uri && tracked.contains(&def.symbol.id))
        .map(|def| def.symbol.id)
        .collect()
}

fn enclosing_loop<'tree>(node: Node<'tree>, stop: Node) -> Option<Node<'tree>> {
    node_and_ancestors(node)
        .take_while(|n| n.id() != stop.id())
        .filter(|n| LOOP_KINDS.contains(&n.kind()))
        .last()
}

struct Captures {
    receiver: Option<Receiver>,
    locals: Vec<CapturedLocal>,
    internals: Vec<InternalLocal>,
}

struct Receiver {
    type_name: String,
    param_name: String,
    rewrites: Vec<ReceiverRewrite>,
}

enum ReceiverRewrite {
    /// Insert `<receiver>.` before a bare implicit-this member reference.
    QualifyAt(usize),
    /// Replace a `this` expression with the receiver parameter name.
    ReplaceThis(Range<usize>),
}

struct CapturedLocal {
    id: SymbolId,
    name: String,
    ty: Type,
    /// An `out` parameter of the enclosing callable: the caller observes every write.
    always_live: bool,
    reads: Vec<usize>,
    writes: Vec<usize>,
    /// Statement ends of whole-value writes that run unconditionally within the selection.
    dominating_write_ends: Vec<usize>,
}

struct InternalLocal {
    id: SymbolId,
    name: String,
}

impl CapturedLocal {
    fn is_written(&self) -> bool {
        !self.writes.is_empty()
    }

    // The entry value cannot reach a read once an unconditional whole-value write precedes them all.
    fn entry_value_unread(&self) -> bool {
        match self.dominating_write_ends.iter().min() {
            Some(&kill) => self.reads.iter().all(|&read| read >= kill),
            None => false,
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn collect_captures(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    roots: &[Node],
    range: &Range<usize>,
    run_block: Option<Node>,
    callable: &Symbol,
    type_context: Option<&TypeContext>,
) -> Option<Captures> {
    // super/parent name a relationship of the enclosing type; no global function can express them.
    let unmovable = &[
        kinds::SUPER_EXPR,
        kinds::PARENT_EXPR,
        kinds::VIRTUAL_PARENT_EXPR,
    ];
    let mut references = Vec::new();
    for root in roots {
        if has_descendant_of_kind(*root, unmovable) {
            return None;
        }
        collect_descendants_of_kind(*root, &[kinds::IDENT, kinds::THIS_EXPR], &mut references);
    }
    references.sort_by_key(Node::start_byte);

    let source = document.source.as_bytes();
    let mut rewrites = Vec::new();
    let mut locals: Vec<CapturedLocal> = Vec::new();
    let mut internals: Vec<InternalLocal> = Vec::new();
    for reference in references {
        if reference.kind() == kinds::THIS_EXPR {
            rewrites.push(ReceiverRewrite::ReplaceThis(reference.byte_range()));
            continue;
        }
        if is_member_slot(reference) {
            continue;
        }
        if reference.utf8_text(source).ok()? == WRAPPED_METHOD_MACRO {
            return None;
        }
        let Some(definition) =
            resolve_definition_at_byte(uri, document, db, reference.start_byte())
        else {
            continue;
        };
        match definition.symbol.kind {
            SymbolKind::Variable | SymbolKind::Parameter
                if definition.uri == uri && definition.symbol.container == Some(callable.id) =>
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
                            && definition.symbol.is_out,
                        reads: Vec::new(),
                        writes: Vec::new(),
                        dominating_write_ends: Vec::new(),
                    });
                    locals.len() - 1
                };
                record_occurrence(&mut locals[index], reference, run_block);
            }
            SymbolKind::Field | SymbolKind::Method | SymbolKind::Event => {
                rewrites.push(ReceiverRewrite::QualifyAt(reference.start_byte()));
            }
            _ => {}
        }
    }
    record_indirect_writes(uri, document, db, roots, &mut locals);

    let receiver = if rewrites.is_empty() {
        None
    } else {
        Some(build_receiver(
            db,
            type_context?,
            &locals,
            &internals,
            rewrites,
        )?)
    };
    Some(Captures {
        receiver,
        locals,
        internals,
    })
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

fn record_indirect_writes(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    roots: &[Node],
    locals: &mut [CapturedLocal],
) {
    let mut sites = Vec::new();
    for root in roots {
        collect_descendants_of_kind(
            *root,
            &[kinds::ASSIGN_OP_EXPR, kinds::FUNC_CALL_EXPR],
            &mut sites,
        );
    }
    for site in sites {
        if site.kind() == kinds::ASSIGN_OP_EXPR {
            let Some(left) = site.child_by_field_name(fields::LEFT) else {
                continue;
            };
            // `pos.x = 1` on a struct local writes the value itself, not a shared object.
            if let (Some(target), Some(base)) = (write_target(left), lvalue_base_ident(left))
                && base.id() != target.id()
            {
                record_value_type_write(uri, document, db, base, locals);
            }
        } else {
            for arg in out_args(uri, document, db, site) {
                if let Some(target) = write_target(arg) {
                    record_write(uri, document, db, target, locals);
                }
            }
            // Array methods mutate in place; a value-param copy would swallow the mutation.
            if let Some(base) = method_call_receiver_base(site) {
                record_value_type_write(uri, document, db, base, locals);
            }
        }
    }
}

fn captured_local_mut<'a>(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    ident: Node,
    locals: &'a mut [CapturedLocal],
) -> Option<&'a mut CapturedLocal> {
    let definition = resolve_definition_at_byte(uri, document, db, ident.start_byte())?;
    if definition.uri != uri {
        return None;
    }
    locals.iter_mut().find(|l| l.id == definition.symbol.id)
}

// Indirect writes go through a reference, so the prior value counts as read too.
fn record_write(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    ident: Node,
    locals: &mut [CapturedLocal],
) {
    if let Some(local) = captured_local_mut(uri, document, db, ident, locals) {
        local.reads.push(ident.start_byte());
        local.writes.push(ident.start_byte());
    }
}

fn record_value_type_write(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    ident: Node,
    locals: &mut [CapturedLocal],
) {
    let Some(definition) = resolve_definition_at_byte(uri, document, db, ident.start_byte()) else {
        return;
    };
    if definition.uri != uri {
        return;
    }
    let Some(local) = locals.iter_mut().find(|l| l.id == definition.symbol.id) else {
        return;
    };
    if is_value_type(&local.ty, db) {
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
        _ => false,
    }
}

fn lvalue_base_ident(expr: Node) -> Option<Node> {
    match expr.kind() {
        kinds::IDENT => Some(expr),
        kinds::MEMBER_ACCESS_EXPR | kinds::NESTED_EXPR => {
            lvalue_base_ident(first_named_child(expr)?)
        }
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

fn build_receiver(
    db: &SymbolDb,
    type_context: &TypeContext,
    locals: &[CapturedLocal],
    internals: &[InternalLocal],
    rewrites: Vec<ReceiverRewrite>,
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
    let taken = |name: &str| {
        locals.iter().any(|l| l.name == name)
            || internals.iter().any(|i| i.name == name)
            || db.find_script_global(name).is_some()
    };
    let param_name = suffixed_unique(&receiver_name(&type_context.name), taken);
    Some(Receiver {
        type_name: type_context.name.clone(),
        param_name,
        rewrites,
    })
}

fn moved_text(source: &str, range: &Range<usize>, captures: &Captures) -> String {
    let Some(receiver) = &captures.receiver else {
        return source[range.clone()].to_string();
    };
    let splices: Vec<Splice> = receiver
        .rewrites
        .iter()
        .map(|rewrite| match rewrite {
            ReceiverRewrite::QualifyAt(at) => Splice {
                range: at - range.start..at - range.start,
                text: format!("{}.", receiver.param_name),
            },
            ReceiverRewrite::ReplaceThis(this) => Splice {
                range: this.start - range.start..this.end - range.start,
                text: receiver.param_name.clone(),
            },
        })
        .collect();
    apply_splices(&source[range.clone()], &splices)
}

fn statement_body(
    source: &str,
    range: &Range<usize>,
    captures: &Captures,
    returned: Option<&CapturedLocal>,
    options: FormatOptions,
) -> String {
    let moved = moved_text(source, range, captures);
    let base = line_indent(source, range.start);
    let mut lines: Vec<String> = Vec::new();
    if let Some(r) = returned {
        lines.push(format!("var {}{}{};", r.name, colon_for(options), r.ty));
    }
    for (i, line) in moved.lines().enumerate() {
        match i {
            0 => lines.push(line.to_string()),
            _ => lines.push(dedent_line(line, base).to_string()),
        }
    }
    if let Some(r) = returned {
        lines.push(format!("return {};", r.name));
    }
    lines.join("\n")
}

fn dedent_line<'a>(line: &'a str, base: &str) -> &'a str {
    if let Some(stripped) = line.strip_prefix(base) {
        return stripped;
    }
    // Mixed tabs/spaces: drop at most the base's width of leading whitespace.
    let mut rest = line;
    for _ in 0..base.len() {
        match rest.strip_prefix([' ', '\t']) {
            Some(stripped) => rest = stripped,
            None => break,
        }
    }
    rest
}

struct FunctionPlan<'a> {
    name: String,
    receiver: Option<&'a Receiver>,
    value_params: Vec<&'a CapturedLocal>,
    out_params: Vec<&'a CapturedLocal>,
    return_type: Type,
    body: String,
}

fn call_expression(plan: &FunctionPlan) -> String {
    let mut args = Vec::new();
    if plan.receiver.is_some() {
        args.push("this");
    }
    args.extend(plan.value_params.iter().map(|l| l.name.as_str()));
    args.extend(plan.out_params.iter().map(|l| l.name.as_str()));
    format!("{}({})", plan.name, args.join(", "))
}

fn render_function(plan: &FunctionPlan, options: FormatOptions) -> String {
    let colon = colon_for(options);
    let mut params = Vec::new();
    if let Some(receiver) = plan.receiver {
        params.push(format!(
            "{}{colon}{}",
            receiver.param_name, receiver.type_name
        ));
    }
    params.extend(
        plan.value_params
            .iter()
            .map(|l| format!("{}{colon}{}", l.name, l.ty)),
    );
    params.extend(
        plan.out_params
            .iter()
            .map(|l| format!("out {}{colon}{}", l.name, l.ty)),
    );
    let params = params.join(", ");
    let return_clause = match &plan.return_type {
        Type::Void => String::new(),
        ty => format!("{colon}{ty}"),
    };
    let body = indent_block(&plan.body, &options);
    format!(
        "function {}({params}){return_clause} {{\n{body}\n}}",
        plan.name
    )
}

fn colon_for(options: FormatOptions) -> &'static str {
    if options.compact_colon { ": " } else { " : " }
}

fn unique_function_name(
    document: &ParsedDocument,
    db: &SymbolDb,
    callable: &Symbol,
    type_context: Option<&TypeContext>,
) -> String {
    // A clash with anything the call-site lookup reaches first would bind the call elsewhere.
    let taken = |name: &str| {
        document
            .symbols
            .children_of(Some(callable.id))
            .any(|s| s.name == name)
            || document.symbols.top_level_by_name(name).is_some()
            || db.find_top_level(name).is_some()
            || db.find_script_global(name).is_some()
            || type_context.is_some_and(|ctx| {
                db.find_member(&ctx.name, name, AccessLevel::Private)
                    .is_some()
            })
    };
    suffixed_unique(DEFAULT_FUNCTION_NAME, taken)
}

fn suffixed_unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
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
