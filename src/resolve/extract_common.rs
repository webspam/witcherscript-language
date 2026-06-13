use std::ops::Range;

use tree_sitter::Node;

use crate::cst::descendants::collect_descendants_of_kind;
use crate::cst::grammar::{arg_slots, call_callee, member_access_member, write_target};
use crate::cst::nav::first_named_child;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::SymbolKind;

use super::definition::callee_params;
use super::symbol_db::SymbolDb;

#[derive(Debug)]
pub struct Splice {
    /// Byte range in the original source this edit replaces; an empty range is a pure insertion.
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Debug)]
pub struct Extraction {
    /// Non-overlapping edits against the original source.
    pub edits: Vec<Splice>,
    pub name: String,
    /// Byte offset in the applied text where the new name starts, for cursor placement.
    pub cursor: usize,
}

impl Extraction {
    pub fn apply(&self, source: &str) -> String {
        apply_splices(source, &self.edits)
    }
}

// Splice rightmost-first so each replace_range leaves earlier byte offsets untouched.
pub(super) fn apply_splices(text: &str, splices: &[Splice]) -> String {
    let mut ordered: Vec<&Splice> = splices.iter().collect();
    ordered.sort_by_key(|s| std::cmp::Reverse(s.range.start));
    let mut applied = text.to_string();
    for splice in ordered {
        applied.replace_range(splice.range.clone(), &splice.text);
    }
    applied
}

// Where `original` lands after the edits apply: shift it past every edit that ends at or before it.
pub(super) fn applied_offset(edits: &[Splice], original: usize) -> usize {
    edits
        .iter()
        .filter(|s| s.range.end <= original)
        .fold(original, |pos, s| pos + s.text.len() - s.range.len())
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

pub(super) const CALLABLE_KINDS: &[SymbolKind] =
    &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event];

pub(super) fn trim_selection(source: &str, selection: Range<usize>) -> Option<Range<usize>> {
    let slice = source.get(selection.clone())?;
    let start = selection.start + (slice.len() - slice.trim_start().len());
    // A trailing `;` is not part of the value; selecting `x;` means the value `x`, not the statement.
    let trimmed = slice.trim_end_matches(|c: char| c.is_whitespace() || c == ';');
    let end = selection.end - (slice.len() - trimmed.len());
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
        .or_else(|| expand_through_new_expr(root, selection))
}

// In `new T in obj`, only the lifetime object is a standalone value; the rest expands to the whole.
fn expand_through_new_expr(root: Node, selection: &Range<usize>) -> Option<Range<usize>> {
    let mut node = root.named_descendant_for_byte_range(selection.start, selection.end)?;
    loop {
        if node.kind() == kinds::NEW_EXPR && !selection_within_lifetime_obj(node, selection) {
            return Some(node.byte_range());
        }
        node = node.parent()?;
    }
}

fn selection_within_lifetime_obj(new_expr: Node, selection: &Range<usize>) -> bool {
    new_expr
        .child_by_field_name(fields::LIFETIME_OBJ)
        .is_some_and(|obj| obj.start_byte() <= selection.start && selection.end <= obj.end_byte())
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

pub(super) fn is_call_callee(node: Node) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == kinds::FUNC_CALL_EXPR)
        .and_then(call_callee)
        .is_some_and(|callee| callee.id() == node.id())
}

pub(super) enum SelectionKind<'tree> {
    Expression {
        node: Node<'tree>,
        range: Range<usize>,
    },
    Statements {
        range: Range<usize>,
    },
}

pub(super) fn classify_selection<'tree>(
    root: Node<'tree>,
    selection: &Range<usize>,
) -> SelectionKind<'tree> {
    let expanded = expand_selection(root, selection).unwrap_or_else(|| selection.clone());
    let Some(node) = exact_expression_at(root, &expanded) else {
        return SelectionKind::Statements {
            range: selection.clone(),
        };
    };
    // An expression that is an entire statement is a statement, not a value to bind or return.
    match node.parent().filter(|p| p.kind() == kinds::EXPR_STMT) {
        Some(stmt) => SelectionKind::Statements {
            range: stmt.byte_range(),
        },
        None => SelectionKind::Expression {
            node,
            range: expanded,
        },
    }
}

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

pub(super) fn out_args<'tree>(
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
