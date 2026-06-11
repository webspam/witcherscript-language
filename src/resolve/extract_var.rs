use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::node_and_ancestors;
use crate::cst::grammar::{
    arg_slots, call_callee, callee_ident, member_access_member, write_target,
};
use crate::cst::walk::{CstVisitor, Visit, walk};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::formatter::{FormatOptions, indent_unit_for, line_indent};
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};
use crate::types::Type;

use super::definition::{callee_params, resolve_definition_at_byte};
use super::inference::infer_type;
use super::symbol_db::SymbolDb;

#[derive(Debug)]
pub struct VariableExtraction {
    /// Byte offset to splice `new_text` into the document, leaving surrounding text unchanged.
    pub insert_at: usize,
    /// Declaration statement plus the newline/indent that joins it to its neighbours.
    pub new_text: String,
    pub replace_range: Range<usize>,
    pub name: String,
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

pub fn extract_variable(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    selection: Range<usize>,
    options: FormatOptions,
) -> Option<VariableExtraction> {
    let source = &document.source;
    let selection = trim_selection(source, selection)?;
    let node = exact_expression_at(document.tree.root_node(), &selection)?;
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
    let statement = declaration_statement(&name, &ty, &source[selection.clone()], options);
    let (insert_at, new_text) = insertion(source, block, &selection, &statement, options)?;
    if hoisting_skips_a_write(uri, document, db, node, block, insert_at, callable.id) {
        return None;
    }
    Some(VariableExtraction {
        insert_at,
        new_text,
        replace_range: selection,
        name,
    })
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

fn is_call_callee(node: Node) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == kinds::FUNC_CALL_EXPR)
        .and_then(call_callee)
        .is_some_and(|callee| callee.id() == node.id())
}

fn enclosing_callable_block(node: Node) -> Option<Node> {
    node_and_ancestors(node).find(|n| {
        n.kind() == kinds::FUNC_BLOCK
            && n.parent()
                .is_some_and(|p| matches!(p.kind(), kinds::FUNC_DECL | kinds::EVENT_DECL))
    })
}

fn declaration_statement(name: &str, ty: &Type, expr: &str, options: FormatOptions) -> String {
    let colon = if options.compact_colon { ": " } else { " : " };
    format!("var {name}{colon}{ty} = {expr};")
}

fn insertion(
    source: &str,
    block: Node,
    selection: &Range<usize>,
    statement: &str,
    options: FormatOptions,
) -> Option<(usize, String)> {
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
            // Inserting after this decl would read the new var before it is declared.
            let indent = line_indent(source, child.start_byte());
            return Some((child.start_byte(), format!("{statement}\n{indent}")));
        }
        last_leading_decl = Some(child);
    }
    if let Some(decl) = last_leading_decl {
        let indent = line_indent(source, decl.start_byte());
        return Some((decl.end_byte(), format!("\n{indent}{statement}")));
    }
    let open_brace = block.child(0).filter(|c| c.kind() == "{")?;
    let unit = indent_unit_for(&options);
    let indent = format!("{}{unit}", line_indent(source, block.start_byte()));
    Some((open_brace.end_byte(), format!("\n{indent}{statement}")))
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

// Scans from the hoist point rather than the selection: in a loop, a textually later write still precedes re-evaluation.
fn hoisting_skips_a_write(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    selection_node: Node,
    block: Node,
    insert_at: usize,
    callable: SymbolId,
) -> bool {
    let tracked = selection_tracked_ids(uri, document, db, selection_node, callable);
    if tracked.is_empty() {
        return false;
    }
    let mut writes = Vec::new();
    collect_nodes_of_kinds(
        block,
        &[kinds::ASSIGN_OP_EXPR, kinds::FUNC_CALL_EXPR],
        &mut writes,
    );
    let is_tracked_write = |target: Node| {
        target.start_byte() >= insert_at
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
    collect_nodes_of_kinds(node, &[kinds::IDENT], &mut idents);
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

fn collect_nodes_of_kinds<'tree>(root: Node<'tree>, kinds: &[&str], out: &mut Vec<Node<'tree>>) {
    struct Collector<'a, 'tree> {
        kinds: &'a [&'a str],
        out: &'a mut Vec<Node<'tree>>,
    }
    impl<'tree> CstVisitor<'tree> for Collector<'_, 'tree> {
        fn enter(&mut self, node: Node<'tree>) -> Visit {
            if self.kinds.contains(&node.kind()) {
                self.out.push(node);
            }
            Visit::Children
        }
    }
    walk(root, &mut Collector { kinds, out });
}

fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
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
