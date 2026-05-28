use tree_sitter::Node;

mod core;
mod declarations;
mod signatures;
mod statements;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationPlacement {
    Preserve,
    OwnLine,
    SameLine,
}

impl AnnotationPlacement {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "ownLine" => Self::OwnLine,
            "sameLine" => Self::SameLine,
            _ => Self::Preserve,
        }
    }
}

impl std::fmt::Display for AnnotationPlacement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preserve => write!(f, "preserve"),
            Self::OwnLine => write!(f, "ownLine"),
            Self::SameLine => write!(f, "sameLine"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FormatOptions {
    pub tab_size: u32,
    pub use_tabs: bool,
    pub line_limit: u32,
    pub compact_colon: bool,
    pub align_member_colons: bool,
    pub annotation_placement: AnnotationPlacement,
}

fn render_expr(node: Node, source: &str) -> String {
    Formatter {
        source,
        indent_unit: String::new(),
        level: 0,
        out: String::new(),
        suppress_next_indent: false,
        line_limit: usize::MAX,
        compact_colon: false,
        align_member_colons: false,
        annotation_placement: AnnotationPlacement::Preserve,
        colon_align_col: None,
    }
    .render_node(node)
}

fn collect_bool_parts(node: Node, source: &str, parts: &mut Vec<(String, Option<&'static str>)>) {
    if node.kind() == "binary_op_expr" {
        if let Some(op_node) = node.child_by_field_name("op") {
            let op_str: Option<&'static str> = match op_node.kind() {
                "binary_op_or" => Some("||"),
                "binary_op_and" => Some("&&"),
                _ => None,
            };
            if let Some(op) = op_str {
                if let (Some(left), Some(right)) = (
                    node.child_by_field_name("left"),
                    node.child_by_field_name("right"),
                ) {
                    collect_bool_parts(left, source, parts);
                    if let Some(last) = parts.last_mut() {
                        last.1 = Some(op);
                    }
                    collect_bool_parts(right, source, parts);
                    return;
                }
            }
        }
    }
    parts.push((render_expr(node, source), None));
}

pub(super) fn split_binary_condition(
    node: Node,
    source: &str,
) -> Vec<(String, Option<&'static str>)> {
    let mut parts = Vec::new();
    collect_bool_parts(node, source, &mut parts);
    parts
}

pub(super) fn try_split_call_args(node: Node, source: &str) -> Option<(String, Vec<String>)> {
    if node.kind() != "func_call_expr" {
        return None;
    }
    let func = node.child_by_field_name("func")?;
    let args_node = node.child_by_field_name("args")?;
    let args: Vec<String> = {
        let mut cursor = args_node.walk();
        args_node
            .children(&mut cursor)
            .filter(|c| c.kind() != ",")
            .map(|c| render_expr(c, source))
            .collect()
    };
    if args.len() <= 1 {
        return None;
    }
    let prefix = render_expr(func, source);
    Some((prefix, args))
}

pub(super) use crate::cst::nav::{child_nodes, named_child_nodes};

pub(super) fn is_alignable_field(node: Node) -> bool {
    if node.kind() != "member_var_decl" || node.is_error() {
        return false;
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "comment" || child.kind() == "annotation" {
            return false;
        }
    }
    true
}

pub(super) fn is_bodiless_callable(node: Node) -> bool {
    if !matches!(node.kind(), "func_decl" | "event_decl") {
        return false;
    }
    let mut c = node.walk();
    let has_block = node.children(&mut c).any(|n| n.kind() == "func_block");
    !has_block
}

pub(super) fn is_expr_node(kind: &str) -> bool {
    matches!(
        kind,
        "array_init_expr"
            | "assign_op_expr"
            | "ternary_cond_expr"
            | "binary_op_expr"
            | "new_expr"
            | "unary_op_expr"
            | "cast_expr"
            | "member_access_expr"
            | "incomplete_member_access_expr"
            | "func_call_expr"
            | "array_expr"
            | "nested_expr"
            | "this_expr"
            | "super_expr"
            | "parent_expr"
            | "virtual_parent_expr"
            | "literal_null"
            | "literal_float"
            | "literal_int"
            | "literal_hex"
            | "literal_bool"
            | "literal_string"
            | "literal_name"
    )
}

/// Renders the parameter list and return type of a callable declaration node as a
/// clean, normalised string — comments stripped, whitespace canonical.
/// Returns `None` if the node has no `func_params` child.
pub fn render_callable_signature(node: Node, source: &str) -> Option<String> {
    let f = Formatter {
        source,
        indent_unit: String::new(),
        level: 0,
        out: String::new(),
        suppress_next_indent: false,
        line_limit: usize::MAX,
        compact_colon: true,
        align_member_colons: false,
        annotation_placement: AnnotationPlacement::Preserve,
        colon_align_col: None,
    };
    f.render_sig(node)
}

pub fn format_document(root: Node, source: &str, options: FormatOptions) -> String {
    let indent_unit = if options.use_tabs {
        "\t".to_string()
    } else {
        " ".repeat(options.tab_size as usize)
    };
    let mut f = Formatter {
        source,
        indent_unit,
        level: 0,
        out: String::with_capacity(source.len()),
        suppress_next_indent: false,
        line_limit: options.line_limit as usize,
        compact_colon: options.compact_colon,
        align_member_colons: options.align_member_colons,
        annotation_placement: options.annotation_placement,
        colon_align_col: None,
    };
    f.format_node(root);
    while f.out.ends_with("\n\n") {
        f.out.pop();
    }
    if !f.out.ends_with('\n') {
        f.out.push('\n');
    }
    f.out
}

struct Formatter<'a> {
    source: &'a str,
    indent_unit: String,
    level: usize,
    out: String,
    suppress_next_indent: bool,
    line_limit: usize,
    compact_colon: bool,
    align_member_colons: bool,
    annotation_placement: AnnotationPlacement,
    colon_align_col: Option<usize>,
}
