use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::{enclosing_callable_block, node_and_ancestors};
use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::grammar::{call_callee, member_access_member, write_target};
use crate::cst::nav::first_named_child;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::formatter::{FormatOptions, indent_unit_for};
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

pub fn extract_function(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    let source = &document.source;
    let selection = trim_selection(source, selection)?;
    let root = document.tree.root_node();
    let selection = expand_selection(root, &selection).unwrap_or(selection);
    let node = exact_expression_at(root, &selection)?;
    extract_expression(uri, document, db, node, selection, options)
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
    let captures = collect_captures(uri, document, db, node, callable, type_context.as_ref())?;
    let name = unique_function_name(document, db, callable, type_context.as_ref());
    let body = format!(
        "return {};",
        moved_text(&document.source, &selection, &captures)
    );
    let function_text = render_function(&name, &captures, &ty, &body, options);
    let insert_at = enclosing_top_level(node)?.end_byte();
    let call_text = format!("{name}({})", call_arguments(&captures).join(", "));
    // A following declaration needs a blank line after the inserted function too.
    let trailing = if document.source[insert_at..].trim().is_empty() {
        ""
    } else {
        "\n"
    };
    let edits = vec![
        Splice {
            range: insert_at..insert_at,
            text: format!("\n\n{function_text}{trailing}"),
        },
        Splice {
            range: selection.clone(),
            text: call_text,
        },
    ];
    let cursor = applied_offset(&edits, selection.start);
    Some(Extraction {
        edits,
        name,
        cursor,
    })
}

fn enclosing_top_level(node: Node) -> Option<Node> {
    node_and_ancestors(node)
        .take_while(|n| n.kind() != kinds::SCRIPT)
        .last()
}

struct Captures {
    receiver: Option<Receiver>,
    locals: Vec<CapturedLocal>,
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
    is_written: bool,
}

fn collect_captures(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    callable: &Symbol,
    type_context: Option<&TypeContext>,
) -> Option<Captures> {
    // super/parent name a relationship of the enclosing type; no global function can express them.
    let unmovable = &[
        kinds::SUPER_EXPR,
        kinds::PARENT_EXPR,
        kinds::VIRTUAL_PARENT_EXPR,
    ];
    if has_descendant_of_kind(node, unmovable) {
        return None;
    }
    let mut references = Vec::new();
    collect_descendants_of_kind(node, &[kinds::IDENT, kinds::THIS_EXPR], &mut references);
    references.sort_by_key(Node::start_byte);

    let source = document.source.as_bytes();
    let mut rewrites = Vec::new();
    let mut locals: Vec<CapturedLocal> = Vec::new();
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
                if definition.uri == uri
                    && definition.symbol.container == Some(callable.id)
                    && locals.iter().all(|l| l.id != definition.symbol.id) =>
            {
                let ty = definition.symbol.type_annotation.clone()?;
                if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
                    return None;
                }
                locals.push(CapturedLocal {
                    id: definition.symbol.id,
                    name: definition.symbol.name.clone(),
                    ty,
                    is_written: false,
                });
            }
            SymbolKind::Field | SymbolKind::Method | SymbolKind::Event => {
                rewrites.push(ReceiverRewrite::QualifyAt(reference.start_byte()));
            }
            _ => {}
        }
    }
    mark_writes(uri, document, db, node, &mut locals);

    let receiver = if rewrites.is_empty() {
        None
    } else {
        Some(build_receiver(db, type_context?, &locals, rewrites)?)
    };
    Some(Captures { receiver, locals })
}

fn is_member_slot(ident: Node) -> bool {
    ident.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            kinds::MEMBER_ACCESS_EXPR | kinds::INCOMPLETE_MEMBER_ACCESS_EXPR
        ) && member_access_member(parent).is_some_and(|member| member.id() == ident.id())
    })
}

fn mark_writes(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    locals: &mut [CapturedLocal],
) {
    let mut sites = Vec::new();
    collect_descendants_of_kind(
        node,
        &[kinds::ASSIGN_OP_EXPR, kinds::FUNC_CALL_EXPR],
        &mut sites,
    );
    for site in sites {
        if site.kind() == kinds::ASSIGN_OP_EXPR {
            let Some(left) = site.child_by_field_name(fields::LEFT) else {
                continue;
            };
            if let Some(target) = write_target(left) {
                mark_written(uri, document, db, target, locals);
            }
            // `pos.x = 1` on a struct local writes the value itself, not a shared object.
            if let Some(base) = lvalue_base_ident(left) {
                mark_value_type_written(uri, document, db, base, locals);
            }
        } else {
            for arg in out_args(uri, document, db, site) {
                if let Some(target) = write_target(arg) {
                    mark_written(uri, document, db, target, locals);
                }
            }
            // Array methods mutate in place; a value-param copy would swallow the mutation.
            if let Some(base) = method_call_receiver_base(site) {
                mark_value_type_written(uri, document, db, base, locals);
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

fn mark_written(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    ident: Node,
    locals: &mut [CapturedLocal],
) {
    if let Some(local) = captured_local_mut(uri, document, db, ident, locals) {
        local.is_written = true;
    }
}

fn mark_value_type_written(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    ident: Node,
    locals: &mut [CapturedLocal],
) {
    let Some(local) = captured_local_mut(uri, document, db, ident, locals) else {
        return;
    };
    if is_value_type(&local.ty, db) {
        local.is_written = true;
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
    let taken =
        |name: &str| locals.iter().any(|l| l.name == name) || db.find_script_global(name).is_some();
    let param_name = suffixed_unique(&receiver_name(&type_context.name), taken);
    Some(Receiver {
        type_name: type_context.name.clone(),
        param_name,
        rewrites,
    })
}

fn moved_text(source: &str, selection: &Range<usize>, captures: &Captures) -> String {
    let Some(receiver) = &captures.receiver else {
        return source[selection.clone()].to_string();
    };
    let splices: Vec<Splice> = receiver
        .rewrites
        .iter()
        .map(|rewrite| match rewrite {
            ReceiverRewrite::QualifyAt(at) => Splice {
                range: at - selection.start..at - selection.start,
                text: format!("{}.", receiver.param_name),
            },
            ReceiverRewrite::ReplaceThis(range) => Splice {
                range: range.start - selection.start..range.end - selection.start,
                text: receiver.param_name.clone(),
            },
        })
        .collect();
    apply_splices(&source[selection.clone()], &splices)
}

fn call_arguments(captures: &Captures) -> Vec<&str> {
    let mut args = Vec::new();
    if captures.receiver.is_some() {
        args.push("this");
    }
    let (values, outs): (Vec<_>, Vec<_>) = captures.locals.iter().partition(|l| !l.is_written);
    args.extend(values.iter().map(|l| l.name.as_str()));
    args.extend(outs.iter().map(|l| l.name.as_str()));
    args
}

fn render_function(
    name: &str,
    captures: &Captures,
    return_type: &Type,
    body: &str,
    options: FormatOptions,
) -> String {
    let colon = if options.compact_colon { ": " } else { " : " };
    let mut params = Vec::new();
    if let Some(receiver) = &captures.receiver {
        params.push(format!(
            "{}{colon}{}",
            receiver.param_name, receiver.type_name
        ));
    }
    let (values, outs): (Vec<_>, Vec<_>) = captures.locals.iter().partition(|l| !l.is_written);
    params.extend(values.iter().map(|l| format!("{}{colon}{}", l.name, l.ty)));
    params.extend(
        outs.iter()
            .map(|l| format!("out {}{colon}{}", l.name, l.ty)),
    );
    let params = params.join(", ");
    let return_clause = match return_type {
        Type::Void => String::new(),
        _ => format!("{colon}{return_type}"),
    };
    let indent = indent_unit_for(&options);
    let body = body
        .lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("function {name}({params}){return_clause} {{\n{body}\n}}")
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
