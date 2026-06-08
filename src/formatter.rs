use tree_sitter::Node;

mod action;
mod core;
mod declarations;
mod if_action;
mod signatures;
mod statements;
mod switch_action;

pub use if_action::{analyze_if, format_if_with_layout, if_stmt_on_keyword, IfLayout, IfToggle};
pub use switch_action::{
    analyze_switch, format_switch_with_layout, switch_stmt_on_keyword, SwitchLayout, SwitchToggle,
};

// One forced layout for the node a code action is rewriting; `None` during ordinary formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutDirective {
    Collapse,
    Expand,
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum AnnotationPlacement {
    #[default]
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

    fn resolve(self, preserve: impl FnOnce() -> bool) -> bool {
        match self {
            Self::SameLine => true,
            Self::OwnLine => false,
            Self::Preserve => preserve(),
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
    pub default_placement: AnnotationPlacement,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            tab_size: 4,
            use_tabs: false,
            line_limit: 100,
            compact_colon: false,
            align_member_colons: false,
            annotation_placement: AnnotationPlacement::default(),
            default_placement: AnnotationPlacement::default(),
        }
    }
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
        default_placement: AnnotationPlacement::Preserve,
        colon_align_col: None,
        comments: Vec::new(),
        comment_cursor: 0,
        layout_directive: None,
    }
    .render_node(node)
}

pub(super) struct BoolPart {
    pub fragment: String,
    pub op: Option<&'static str>,
    // The author put a newline between this operand and the next one in the source.
    pub break_after: bool,
}

fn collect_bool_parts(node: Node, source: &str, parts: &mut Vec<BoolPart>) {
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
                        last.op = Some(op);
                        last.break_after = right.start_position().row > left.end_position().row;
                    }
                    collect_bool_parts(right, source, parts);
                    return;
                }
            }
        }
    }
    parts.push(BoolPart {
        fragment: render_expr(node, source),
        op: None,
        break_after: false,
    });
}

pub(super) fn split_binary_condition(node: Node, source: &str) -> Vec<BoolPart> {
    let mut parts = Vec::new();
    collect_bool_parts(node, source, &mut parts);
    parts
}

pub(super) fn chain_fully_broken(parts: &[BoolPart]) -> bool {
    // The last operand has no successor, so break_after is only meaningful on the rest.
    parts.len() > 1 && parts[..parts.len() - 1].iter().all(|p| p.break_after)
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
    let has_comment_or_annotation = node
        .children(&mut c)
        .any(|n| matches!(n.kind(), "comment" | "annotation"));
    !has_comment_or_annotation
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

fn collect_comments(root: Node) -> Vec<Node> {
    let mut comments = Vec::new();
    let mut cursor = root.walk();
    loop {
        if cursor.node().kind() == "comment" {
            comments.push(cursor.node());
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return comments;
            }
        }
    }
}

/// Renders the parameter list and return type of a callable declaration node as a
/// clean, normalised string - comments stripped, whitespace canonical.
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
        default_placement: AnnotationPlacement::Preserve,
        colon_align_col: None,
        comments: Vec::new(),
        comment_cursor: 0,
        layout_directive: None,
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
        default_placement: options.default_placement,
        colon_align_col: None,
        comments: collect_comments(root),
        comment_cursor: 0,
        layout_directive: None,
    };
    f.format_node(root);
    f.flush_comments_before(usize::MAX);
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
    default_placement: AnnotationPlacement,
    colon_align_col: Option<usize>,
    // Source-ordered comments; the sweep emits each just before the next node past it.
    comments: Vec<Node<'a>>,
    comment_cursor: usize,
    // When set, the targeted node is forced to one layout instead of mirroring the source rows.
    layout_directive: Option<LayoutDirective>,
}
