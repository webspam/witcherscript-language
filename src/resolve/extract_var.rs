use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::{enclosing_callable_block, node_and_ancestors};
use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::grammar::{arg_slots, call_callee, callee_ident, member_access_member};
use crate::cst::if_stmt::{if_chain_above, mutually_exclusive_branches};
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::formatter::{FormatOptions, indent_block, indent_unit_for, line_indent};
use crate::strings::lowercase_first;
use crate::symbols::{AccessLevel, Symbol, SymbolKind};
use crate::types::Type;

use super::body_model::{BodyModel, WriteKinds};
use super::definition::callee_params;
use super::extract_common::{
    CALLABLE_KINDS, Extraction, SelectionKind, Splice, applied_offset, classify_selection,
    is_call_callee, trim_selection,
};
use super::inference::infer_type;
use super::symbol_db::SymbolDb;

const STATEMENT_KINDS: &[&str] = &[
    kinds::LOCAL_VAR_DECL_STMT,
    kinds::FOR_STMT,
    kinds::WHILE_STMT,
    kinds::DO_WHILE_STMT,
    kinds::IF_STMT,
    kinds::SWITCH_STMT,
    kinds::BREAK_STMT,
    kinds::CONTINUE_STMT,
    kinds::RETURN_STMT,
    kinds::DELETE_STMT,
    kinds::FUNC_BLOCK,
    kinds::EXPR_STMT,
    kinds::NOP,
];

pub fn extract_variable(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<Extraction> {
    let source = &document.source;
    let selection = trim_selection(source, selection)?;
    let root = document.tree.root_node();
    let SelectionKind::Expression {
        node,
        range: selection,
    } = classify_selection(root, &selection)
    else {
        // A whole expression statement is not a value to bind; replacing it would leave a bare `name;`.
        return None;
    };
    if is_call_callee(node) {
        // A bare reference to the callee is a function reference, which WitcherScript has no values for.
        return None;
    }
    let block = enclosing_callable_block(node)?;
    let callable = document
        .symbols
        .enclosing_symbol_at(selection.start, CALLABLE_KINDS)?;
    let ty = infer_type(uri, document, db, node, selection.start);
    if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
        return None;
    }
    let name = unique_name(&name_base(uri, document, db, node), document, db, callable);
    let expr = &source[selection.clone()];

    let model = BodyModel::enclosing(uri, document, db, selection.start)?;
    let value = selection.clone();
    let reads_nonlocal =
        has_descendant_of_kind(node, &[kinds::FUNC_CALL_EXPR]) || model.references_field(&value);

    // A frozen top-of-block value is stale once a read is written before the expression re-evaluates:
    // before it textually, or anywhere in an enclosing loop body (which re-runs the expression).
    let loop_end = enclosing_loop_end(node, block);
    let hoist_end = loop_end.unwrap_or(selection.start);
    let cannot_hoist_initializer = |window: Range<usize>| {
        model.operand_written_in(&value, &window, WriteKinds::Reassignment)
            || (reads_nonlocal
                && overridable_call_precedes(node, block, window, loop_end.is_some()))
    };

    match decl_site(source, block, &selection, options)? {
        DeclSite::AboveLeadingDecl { at, indent } => {
            if cannot_hoist_initializer(at..hoist_end) {
                return None;
            }
            let stmt = declaration_statement(&name, &ty, expr, options);
            Some(single_insert(
                at,
                format!("{stmt}\n{indent}"),
                selection,
                name,
            ))
        }
        DeclSite::TopOfBlock { at, indent } => {
            if !cannot_hoist_initializer(at..hoist_end) {
                let stmt = declaration_statement(&name, &ty, expr, options);
                return Some(single_insert(
                    at,
                    format!("\n{indent}{stmt}"),
                    selection,
                    name,
                ));
            }
            // Hoisting the whole decl would skip a write; split it so the computation stays in place.
            let slot = assign_slot(node)?;
            let statement = slot.statement();
            if model.operand_written_in(
                &value,
                &(statement.start_byte()..selection.start),
                WriteKinds::Reassignment,
            ) {
                return None;
            }
            let decl = format!(
                "\n{indent}{}",
                uninitialised_declaration(&name, &ty, options)
            );
            match slot {
                AssignSlot::BeforeStatement(statement) => Some(split_before(
                    at,
                    decl,
                    statement.start_byte(),
                    source,
                    name,
                    expr,
                    selection,
                )),
                AssignSlot::WrapBraceless(statement) => match pre_chain_head(statement, node) {
                    Some(head) => Some(split_before(
                        at,
                        decl,
                        head.start_byte(),
                        source,
                        name,
                        expr,
                        selection,
                    )),
                    None => Some(split_braceless(
                        Splice {
                            range: at..at,
                            text: decl,
                        },
                        statement,
                        expr,
                        source,
                        options,
                        selection,
                        name,
                    )),
                },
            }
        }
    }
}

fn single_insert(at: usize, text: String, selection: Range<usize>, name: String) -> Extraction {
    let anchor = selection.start;
    let edits = vec![
        Splice {
            range: at..at,
            text,
        },
        Splice {
            range: selection,
            text: name.clone(),
        },
    ];
    let cursor = applied_offset(&edits, anchor);
    Extraction {
        edits,
        name,
        cursor,
    }
}

fn split(
    decl_at: usize,
    decl_text: String,
    assign_at: usize,
    assign_text: String,
    selection: Range<usize>,
    name: String,
) -> Extraction {
    let anchor = selection.start;
    let edits = vec![
        Splice {
            range: decl_at..decl_at,
            text: decl_text,
        },
        Splice {
            range: assign_at..assign_at,
            text: assign_text,
        },
        Splice {
            range: selection,
            text: name.clone(),
        },
    ];
    let cursor = applied_offset(&edits, anchor);
    Extraction {
        edits,
        name,
        cursor,
    }
}

fn split_before(
    decl_at: usize,
    decl_text: String,
    assign_at: usize,
    source: &str,
    name: String,
    expr: &str,
    selection: Range<usize>,
) -> Extraction {
    let assign_indent = line_indent(source, assign_at);
    let assign = format!("{name} = {expr};\n{assign_indent}");
    split(decl_at, decl_text, assign_at, assign, selection, name)
}

// Wrap a braceless statement in a synthesised block; the whole region is one rendered replacement.
fn split_braceless(
    decl: Splice,
    statement: Node,
    expr: &str,
    source: &str,
    options: FormatOptions,
    selection: Range<usize>,
    name: String,
) -> Extraction {
    let stmt_start = statement.start_byte();
    let stmt_end = statement.end_byte();

    let body = &source[stmt_start..stmt_end];
    let rel = (selection.start - stmt_start)..(selection.end - stmt_start);

    let indent = line_indent(source, statement.start_byte()).to_string();
    let substituted = format!(
        "{indent}{name} = {expr};\n{indent}{}{name}{}",
        &body[..rel.start],
        &body[rel.end..]
    );
    let block = format!("{{\n{}\n{indent}}}", indent_block(&substituted, &options));

    // `decl` inserts above the block, we need to count to symbol loc
    let body_indent = indent_unit_for(&options).len() + indent.len();
    let cursor = stmt_start + decl.text.len() + "{\n".len() + body_indent;
    Extraction {
        edits: vec![
            decl,
            Splice {
                range: stmt_start..stmt_end,
                text: block,
            },
        ],
        name,
        cursor,
    }
}

fn declaration_statement(name: &str, ty: &Type, expr: &str, options: FormatOptions) -> String {
    let colon = options.colon.separator();
    format!("var {name}{colon}{ty} = {expr};")
}

fn uninitialised_declaration(name: &str, ty: &Type, options: FormatOptions) -> String {
    let colon = options.colon.separator();
    format!("var {name}{colon}{ty};")
}

enum DeclSite {
    // Selection sits inside a leading var-decl initializer; the new decl must precede that decl, with init.
    AboveLeadingDecl { at: usize, indent: String },
    // Top of the function block, after any leading decls or the open brace.
    TopOfBlock { at: usize, indent: String },
}

fn decl_site(
    source: &str,
    block: Node,
    selection: &Range<usize>,
    options: FormatOptions,
) -> Option<DeclSite> {
    let mut last_leading_decl: Option<Node> = None;
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if !child.is_named() || matches!(child.kind(), kinds::COMMENT | kinds::NOP) {
            continue;
        }
        if child.kind() != kinds::LOCAL_VAR_DECL_STMT {
            break;
        }
        if child.start_byte() <= selection.start && selection.end <= child.end_byte() {
            let indent = line_indent(source, child.start_byte()).to_string();
            return Some(DeclSite::AboveLeadingDecl {
                at: child.start_byte(),
                indent,
            });
        }
        last_leading_decl = Some(child);
    }
    if let Some(decl) = last_leading_decl {
        let indent = line_indent(source, decl.start_byte()).to_string();
        return Some(DeclSite::TopOfBlock {
            at: decl.end_byte(),
            indent,
        });
    }
    let open_brace = block.child(0).filter(|c| c.kind() == "{")?;
    let unit = indent_unit_for(&options);
    let indent = format!("{}{unit}", line_indent(source, block.start_byte()));
    Some(DeclSite::TopOfBlock {
        at: open_brace.end_byte(),
        indent,
    })
}

// End byte of the outermost loop enclosing the selection within `block`; None if it sits in no loop.
fn enclosing_loop_end(node: Node, block: Node) -> Option<usize> {
    node_and_ancestors(node)
        .take_while(|n| n.id() != block.id())
        .filter(|n| {
            matches!(
                n.kind(),
                kinds::FOR_STMT | kinds::WHILE_STMT | kinds::DO_WHILE_STMT
            )
        })
        .last()
        .map(|loop_node| loop_node.end_byte())
}

const BRACELESS_HOST_KINDS: &[&str] = &[
    kinds::IF_STMT,
    kinds::FOR_STMT,
    kinds::WHILE_STMT,
    kinds::DO_WHILE_STMT,
];

enum AssignSlot<'tree> {
    BeforeStatement(Node<'tree>),
    // Braceless control-flow body (or the inner `if` of an `else if`): hosting the assignment needs braces.
    WrapBraceless(Node<'tree>),
}

impl<'tree> AssignSlot<'tree> {
    fn statement(&self) -> Node<'tree> {
        match self {
            AssignSlot::BeforeStatement(s) | AssignSlot::WrapBraceless(s) => *s,
        }
    }
}

// Where the in-place split assignment lands. A braceless control-flow body interposes no func_block,
// so the statement's parent being a control-flow node identifies the braces-needed case.
fn assign_slot(node: Node) -> Option<AssignSlot> {
    let statement = node_and_ancestors(node).find(|n| STATEMENT_KINDS.contains(&n.kind()))?;
    let parent = statement.parent()?;
    match parent.kind() {
        kinds::FUNC_BLOCK => Some(AssignSlot::BeforeStatement(statement)),
        k if BRACELESS_HOST_KINDS.contains(&k) => Some(AssignSlot::WrapBraceless(statement)),
        _ => None,
    }
}

// An else-if assignment can precede the chain's first `if` (no block needed) only if nothing reached
// on the way - the extracted expression and every preceding condition - can mutate the reads.
fn pre_chain_head<'tree>(statement: Node<'tree>, expr: Node) -> Option<Node<'tree>> {
    let (head, preceding_conditions) = if_chain_above(statement)?;
    if head.parent()?.kind() != kinds::FUNC_BLOCK {
        return None;
    }
    if has_side_effect(expr) || preceding_conditions.iter().any(|c| has_side_effect(*c)) {
        return None;
    }
    Some(head)
}

fn has_side_effect(node: Node) -> bool {
    has_descendant_of_kind(node, &[kinds::FUNC_CALL_EXPR, kinds::ASSIGN_OP_EXPR])
}

fn name_base(uri: &str, document: &ParsedDocument, db: &SymbolDb, node: Node) -> String {
    if let Some(parameter) = parameter_slot_name(uri, document, db, node) {
        return parameter;
    }
    let source = document.source.as_bytes();
    let derived = match node.kind() {
        kinds::FUNC_CALL_EXPR => call_callee(node)
            .and_then(callee_ident)
            .and_then(|ident| ident.utf8_text(source).ok())
            .map(lowercase_first),
        kinds::MEMBER_ACCESS_EXPR => member_access_member(node)
            .filter(|member| member.kind() == kinds::IDENT)
            .and_then(|member| member.utf8_text(source).ok())
            .map(str::to_string),
        _ => None,
    };
    derived.unwrap_or_else(|| "newVar".to_string())
}

fn parameter_slot_name(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
) -> Option<String> {
    let args = node
        .parent()
        .filter(|p| p.kind() == kinds::FUNC_CALL_ARGS)?;
    let call = args
        .parent()
        .filter(|p| p.kind() == kinds::FUNC_CALL_EXPR)?;
    let index = arg_slots(call)?
        .iter()
        .position(|slot| slot.id() == node.id())?;
    callee_params(uri, document, db, call)?
        .get(index)
        .map(|parameter| parameter.name.clone())
}

// Calls in an earlier initializer run before our decl regardless, so only those from the insertion
// point (window start, after the last leading var decl) up to the hoist end can change the reads.
fn overridable_call_precedes(node: Node, block: Node, window: Range<usize>, in_loop: bool) -> bool {
    let mut calls = Vec::new();
    collect_descendants_of_kind(block, &[kinds::FUNC_CALL_EXPR], &mut calls);
    calls.iter().any(|call| {
        // A loop re-runs both branches, so the then/else exclusion only holds outside one.
        window.contains(&call.start_byte())
            && (in_loop || !mutually_exclusive_branches(*call, node))
    })
}

fn unique_name(base: &str, document: &ParsedDocument, db: &SymbolDb, callable: &Symbol) -> String {
    let taken: HashSet<&str> = document
        .symbols
        .children_of(Some(callable.id))
        .filter(|s| matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter))
        .map(|s| s.name.as_str())
        .collect();
    let class = callable
        .container
        .and_then(|id| document.symbols.by_id(id))
        .filter(|c| c.kind.is_instantiable());
    // Mirror the shadowing diagnostics: the generated local must not shadow a class field or engine global.
    let shadows = |name: &str| {
        db.find_script_global(name).is_some()
            || class.is_some_and(|c| {
                db.find_member(&c.name, name, AccessLevel::Private)
                    .is_some_and(|d| d.symbol.kind == SymbolKind::Field)
            })
    };
    if !taken.contains(base) && !shadows(base) {
        return base.to_string();
    }
    let mut suffix = 1usize;
    loop {
        let candidate = format!("{base}{suffix}");
        if !taken.contains(candidate.as_str()) && !shadows(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}
