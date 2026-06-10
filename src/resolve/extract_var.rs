use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::node_and_ancestors;
use crate::cst::grammar::{arg_slots, call_callee, callee_ident, member_access_member};
use crate::cst::kinds;
use crate::document::ParsedDocument;
use crate::formatter::FormatOptions;
use crate::symbols::{SymbolId, SymbolKind};
use crate::types::Type;

use super::definition::resolve_definition_at_byte;
use super::inference::infer_type;
use super::symbol_db::SymbolDb;

#[derive(Debug)]
pub struct VariableExtraction {
    /// Byte offset to splice `new_text` into the document, leaving surrounding text unchanged.
    pub insert_at: usize,
    /// Declaration statement plus the newline/indent that joins it to its neighbours.
    pub new_text: String,
    /// Byte range of the selected expression, to be replaced by `name`.
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
    let block = enclosing_callable_block(node)?;
    let callable = document
        .symbols
        .enclosing_symbol_at(selection.start, CALLABLE_KINDS)?;
    let ty = infer_type(uri, document, db, node, selection.start);
    if matches!(ty, Type::Unknown | Type::Null | Type::Void) {
        return None;
    }
    let name = unique_name(&name_base(uri, document, db, node), document, callable.id);
    let statement = declaration_statement(&name, &ty, &source[selection.clone()], options);
    let (insert_at, new_text) = insertion(source, block, &selection, &statement, options)?;
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

// A selection inside a leading decl's initializer inserts before that decl, else the new var is read before declared.
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
    let unit = if options.use_tabs {
        "\t".to_string()
    } else {
        " ".repeat(options.tab_size as usize)
    };
    let indent = format!("{}{unit}", line_indent(source, block.start_byte()));
    Some((open_brace.end_byte(), format!("\n{indent}{statement}")))
}

fn line_indent(source: &str, byte: usize) -> &str {
    let line_start = source[..byte].rfind('\n').map_or(0, |i| i + 1);
    let line = &source[line_start..byte];
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..indent_len]
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
    let ident = callee_ident(call_callee(call)?)?;
    let definition = resolve_definition_at_byte(uri, document, db, ident.start_byte())
        .filter(|def| def.symbol.kind.is_callable())?;
    db.full_parameters_of(&definition.uri, definition.symbol.id)
        .get(index)
        .map(|parameter| parameter.name.clone())
}

fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn unique_name(base: &str, document: &ParsedDocument, callable: SymbolId) -> String {
    let taken: Vec<&str> = document
        .symbols
        .children_of(Some(callable))
        .filter(|s| matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter))
        .map(|s| s.name.as_str())
        .collect();
    if !taken.contains(&base) {
        return base.to_string();
    }
    let mut suffix = 1usize;
    loop {
        let candidate = format!("{base}{suffix}");
        if !taken.contains(&candidate.as_str()) {
            return candidate;
        }
        suffix += 1;
    }
}
