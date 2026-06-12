use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::{enclosing_callable_block, node_and_ancestors};
use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::grammar::{
    arg_slots, call_callee, callee_ident, member_access_member, write_target,
};
use crate::cst::if_stmt::{if_chain_above, mutually_exclusive_branches};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::formatter::{FormatOptions, indent_unit_for, line_indent};
use crate::strings::lowercase_first;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};
use crate::types::Type;

use super::definition::{callee_params, resolve_definition_at_byte};
use super::inference::infer_type;
use super::symbol_db::SymbolDb;

#[derive(Debug)]
pub struct Splice {
    /// Byte range in the original source this edit replaces; an empty range is a pure insertion.
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Debug)]
pub struct VariableExtraction {
    /// Non-overlapping edits against the original source: the declaration insert, an optional
    /// in-place assignment insert (split form), and the selection-to-name replacement.
    pub edits: Vec<Splice>,
    pub name: String,
    /// Byte offset in the original source where the new name lands (selection start), for cursor placement.
    pub name_anchor: usize,
}

impl VariableExtraction {
    // Splice rightmost-first so each replace_range leaves earlier byte offsets untouched.
    pub fn apply(&self, source: &str) -> String {
        let mut splices: Vec<&Splice> = self.edits.iter().collect();
        splices.sort_by_key(|s| std::cmp::Reverse(s.range.start));
        let mut applied = source.to_string();
        for splice in splices {
            applied.replace_range(splice.range.clone(), &splice.text);
        }
        applied
    }
}

const EXTRACTABLE_KINDS: &[&str] = &[
    kinds::BINARY_OP_EXPR,
    kinds::UNARY_OP_EXPR,
    kinds::FUNC_CALL_EXPR,
    kinds::MEMBER_ACCESS_EXPR,
    kinds::ARRAY_EXPR,
    kinds::NESTED_EXPR,
    kinds::CAST_EXPR,
    kinds::NEW_EXPR,
    kinds::IDENT,
    kinds::LITERAL_INT,
    kinds::LITERAL_HEX,
    kinds::LITERAL_FLOAT,
    kinds::LITERAL_BOOL,
    kinds::LITERAL_STRING,
    kinds::LITERAL_NAME,
];

const CALLABLE_KINDS: &[SymbolKind] =
    &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event];

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
) -> Option<VariableExtraction> {
    let source = &document.source;
    let selection = trim_selection(source, selection)?;
    let root = document.tree.root_node();
    let selection = expand_selection(root, &selection).unwrap_or(selection);
    let node = exact_expression_at(root, &selection)?;
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

    // A frozen top-of-block value is stale once a read is written before the expression re-evaluates:
    // before it textually, or anywhere in an enclosing loop body (which re-runs the expression).
    let loop_end = enclosing_loop_end(node, block);
    let hoist_end = loop_end.unwrap_or(selection.start);
    let cannot_hoist_initializer = |window: Range<usize>| {
        tracked_write_in_window(uri, document, db, node, block, window.clone(), callable.id)
            || (reads_nonlocal_state(uri, document, db, node)
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
            let window = statement.start_byte()..selection.start;
            if tracked_write_in_window(uri, document, db, node, block, window, callable.id) {
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

fn single_insert(
    at: usize,
    text: String,
    selection: Range<usize>,
    name: String,
) -> VariableExtraction {
    let name_anchor = selection.start;
    VariableExtraction {
        edits: vec![
            Splice {
                range: at..at,
                text,
            },
            Splice {
                range: selection,
                text: name.clone(),
            },
        ],
        name,
        name_anchor,
    }
}

fn split(
    decl_at: usize,
    decl_text: String,
    assign_at: usize,
    assign_text: String,
    selection: Range<usize>,
    name: String,
) -> VariableExtraction {
    let name_anchor = selection.start;
    VariableExtraction {
        edits: vec![
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
        ],
        name,
        name_anchor,
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
) -> VariableExtraction {
    let assign_indent = line_indent(source, assign_at);
    let assign = format!("{name} = {expr};\n{assign_indent}");
    split(decl_at, decl_text, assign_at, assign, selection, name)
}

// Wrap a braceless statement in a synthesised block holding the assignment, keeping it in place.
fn split_braceless(
    decl: Splice,
    statement: Node,
    expr: &str,
    source: &str,
    options: FormatOptions,
    selection: Range<usize>,
    name: String,
) -> VariableExtraction {
    let indent = line_indent(source, statement.start_byte()).to_string();
    let unit = indent_unit_for(&options);
    let open = format!("{{\n{indent}{unit}{name} = {expr};\n{indent}{unit}");
    let close = format!("\n{indent}}}");
    let name_anchor = selection.start;
    let stmt_start = statement.start_byte();
    let stmt_end = statement.end_byte();
    VariableExtraction {
        edits: vec![
            decl,
            Splice {
                range: stmt_start..stmt_start,
                text: open,
            },
            Splice {
                range: stmt_end..stmt_end,
                text: close,
            },
            Splice {
                range: selection,
                text: name.clone(),
            },
        ],
        name,
        name_anchor,
    }
}

fn trim_selection(source: &str, selection: Range<usize>) -> Option<Range<usize>> {
    let slice = source.get(selection.clone())?;
    let start = selection.start + (slice.len() - slice.trim_start().len());
    let end = selection.end - (slice.len() - slice.trim_end().len());
    (start < end).then_some(start..end)
}

// The smallest covering node can be a leaf inside same-range wrappers; keep the outermost extractable one.
fn exact_expression_at<'tree>(root: Node<'tree>, selection: &Range<usize>) -> Option<Node<'tree>> {
    let mut node = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    if node.byte_range() != *selection {
        return None;
    }
    let mut best = None;
    loop {
        if EXTRACTABLE_KINDS.contains(&node.kind()) {
            best = Some(node);
        }
        match node.parent() {
            Some(parent) if parent.byte_range() == *selection => node = parent,
            _ => return best,
        }
    }
}

// A selection landing on a structural boundary expands to the whole value rather than refusing.
fn expand_selection(root: Node, selection: &Range<usize>) -> Option<Range<usize>> {
    expand_through_logical_operator(root, selection)
        .or_else(|| expand_through_postfix_chain(root, selection))
}

const POSTFIX_CHAIN_KINDS: &[&str] = &[
    kinds::MEMBER_ACCESS_EXPR,
    kinds::FUNC_CALL_EXPR,
    kinds::ARRAY_EXPR,
];

// Promoting a touched method reference to its call yields a value, not an uncallable handle.
fn expand_through_postfix_chain(root: Node, selection: &Range<usize>) -> Option<Range<usize>> {
    let mut node = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    loop {
        if POSTFIX_CHAIN_KINDS.contains(&node.kind())
            && selection_touches_separator(node, selection)
        {
            return Some(promote_callee(node).byte_range());
        }
        node = node.parent()?;
    }
}

fn selection_touches_separator(node: Node, selection: &Range<usize>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        !child.is_named()
            && child.start_byte() < selection.end
            && selection.start < child.end_byte()
    })
}

fn promote_callee(node: Node) -> Node {
    node.parent()
        .filter(|parent| parent.kind() == kinds::FUNC_CALL_EXPR)
        .filter(|parent| {
            parent
                .child_by_field_name(fields::FUNC)
                .is_some_and(|func| func.id() == node.id())
        })
        .unwrap_or(node)
}

// Extracting both operands the touched `||`/`&&` joins keeps short-circuit evaluation intact.
fn expand_through_logical_operator(root: Node, selection: &Range<usize>) -> Option<Range<usize>> {
    let mut node = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    loop {
        if node.kind() == kinds::BINARY_OP_EXPR && selection_touches_logical_op(node, selection) {
            return Some(node.byte_range());
        }
        node = node.parent()?;
    }
}

fn selection_touches_logical_op(binary: Node, selection: &Range<usize>) -> bool {
    let Some(op) = binary.child_by_field_name(fields::OP) else {
        return false;
    };
    matches!(op.kind(), kinds::BINARY_OP_OR | kinds::BINARY_OP_AND)
        && op.start_byte() < selection.end
        && selection.start < op.end_byte()
}

fn is_call_callee(node: Node) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == kinds::FUNC_CALL_EXPR)
        .and_then(call_callee)
        .is_some_and(|callee| callee.id() == node.id())
}

fn declaration_statement(name: &str, ty: &Type, expr: &str, options: FormatOptions) -> String {
    let colon = if options.compact_colon { ": " } else { " : " };
    format!("var {name}{colon}{ty} = {expr};")
}

fn uninitialised_declaration(name: &str, ty: &Type, options: FormatOptions) -> String {
    let colon = if options.compact_colon { ": " } else { " : " };
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

// A write inside `window` to any id the selection reads makes moving the computation there unsafe.
fn tracked_write_in_window(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    selection_node: Node,
    block: Node,
    window: Range<usize>,
    callable: SymbolId,
) -> bool {
    let tracked = selection_tracked_ids(uri, document, db, selection_node, callable);
    if tracked.is_empty() {
        return false;
    }
    let mut writes = Vec::new();
    collect_descendants_of_kind(
        block,
        &[kinds::ASSIGN_OP_EXPR, kinds::FUNC_CALL_EXPR],
        &mut writes,
    );
    let is_tracked_write = |target: Node| {
        window.contains(&target.start_byte())
            && resolved_write_tracked_id(uri, document, db, target, callable)
                .is_some_and(|id| tracked.contains(&id))
    };
    writes.iter().any(|node| match node.kind() {
        kinds::ASSIGN_OP_EXPR => node
            .child_by_field_name(fields::LEFT)
            .and_then(write_target)
            .is_some_and(is_tracked_write),
        _ => out_args(uri, document, db, *node)
            .into_iter()
            .filter_map(write_target)
            .any(is_tracked_write),
    })
}

// Any function/method may be redirected by @wrapMethod/@replaceMethod beyond our visibility, so a
// call that can run before the expression could mutate the non-local state it reads.
fn reads_nonlocal_state(uri: &str, document: &ParsedDocument, db: &SymbolDb, node: Node) -> bool {
    if has_descendant_of_kind(node, &[kinds::FUNC_CALL_EXPR]) {
        return true;
    }
    let mut idents = Vec::new();
    collect_descendants_of_kind(node, &[kinds::IDENT], &mut idents);
    idents.iter().any(|ident| {
        resolve_definition_at_byte(uri, document, db, ident.start_byte())
            .is_some_and(|def| def.symbol.kind == SymbolKind::Field)
    })
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
        .filter(|(parameter, _)| parameter.is_out)
        .map(|(_, arg)| arg)
        .collect()
}

fn selection_tracked_ids(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    callable: SymbolId,
) -> HashSet<SymbolId> {
    let mut idents = Vec::new();
    collect_descendants_of_kind(node, &[kinds::IDENT], &mut idents);
    idents
        .iter()
        .filter_map(|ident| resolved_write_tracked_id(uri, document, db, *ident, callable))
        .collect()
}

// Same-file only: SymbolId is an index into one document's symbols, so cross-file ids cannot be compared.
fn resolved_write_tracked_id(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    ident: Node,
    callable: SymbolId,
) -> Option<SymbolId> {
    let definition = resolve_definition_at_byte(uri, document, db, ident.start_byte())?;
    if definition.uri != uri {
        return None;
    }
    let symbol = definition.symbol;
    let tracked = match symbol.kind {
        SymbolKind::Variable | SymbolKind::Parameter => symbol.container == Some(callable),
        SymbolKind::Field => true,
        _ => false,
    };
    tracked.then_some(symbol.id)
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
