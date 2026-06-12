use tree_sitter::Node;

use crate::cst::{fields, kinds};

mod action;
mod core;
mod declarations;
mod if_action;
mod signatures;
mod statements;
mod switch_action;

pub(crate) use action::{indent_block, indent_unit_for, line_indent};
pub use if_action::{IfLayout, IfToggle, analyze_if, if_chain_at, rewrite_if_layout};
pub use switch_action::{
    SwitchLayout, SwitchToggle, analyze_switch, rewrite_switch_layout, switch_stmt_at,
};

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
    if node.kind() == kinds::BINARY_OP_EXPR
        && let Some(op_node) = node.child_by_field_name(fields::OP)
    {
        let op_str: Option<&'static str> = match op_node.kind() {
            kinds::BINARY_OP_OR => Some("||"),
            kinds::BINARY_OP_AND => Some("&&"),
            _ => None,
        };
        if let Some(op) = op_str
            && let (Some(left), Some(right)) = (
                node.child_by_field_name(fields::LEFT),
                node.child_by_field_name(fields::RIGHT),
            )
        {
            collect_bool_parts(left, source, parts);
            if let Some(last) = parts.last_mut() {
                last.op = Some(op);
                last.break_after = right.start_position().row > left.end_position().row;
            }
            collect_bool_parts(right, source, parts);
            return;
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
    if node.kind() != kinds::FUNC_CALL_EXPR {
        return None;
    }
    let func = node.child_by_field_name(fields::FUNC)?;
    let args_node = node.child_by_field_name(fields::ARGS)?;
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
    if node.kind() != kinds::MEMBER_VAR_DECL || node.is_error() {
        return false;
    }
    let mut c = node.walk();
    let has_comment_or_annotation = node
        .children(&mut c)
        .any(|n| matches!(n.kind(), kinds::COMMENT | kinds::ANNOTATION));
    !has_comment_or_annotation
}

pub(super) fn is_bodiless_callable(node: Node) -> bool {
    if !matches!(node.kind(), kinds::FUNC_DECL | kinds::EVENT_DECL) {
        return false;
    }
    let mut c = node.walk();
    let has_block = node.children(&mut c).any(|n| n.kind() == kinds::FUNC_BLOCK);
    !has_block
}

pub(super) fn is_expr_node(kind: &str) -> bool {
    matches!(
        kind,
        kinds::ARRAY_INIT_EXPR
            | kinds::ASSIGN_OP_EXPR
            | kinds::TERNARY_COND_EXPR
            | kinds::BINARY_OP_EXPR
            | kinds::NEW_EXPR
            | kinds::UNARY_OP_EXPR
            | kinds::CAST_EXPR
            | kinds::MEMBER_ACCESS_EXPR
            | kinds::INCOMPLETE_MEMBER_ACCESS_EXPR
            | kinds::FUNC_CALL_EXPR
            | kinds::ARRAY_EXPR
            | kinds::NESTED_EXPR
            | kinds::THIS_EXPR
            | kinds::SUPER_EXPR
            | kinds::PARENT_EXPR
            | kinds::VIRTUAL_PARENT_EXPR
            | kinds::LITERAL_NULL
            | kinds::LITERAL_FLOAT
            | kinds::LITERAL_INT
            | kinds::LITERAL_HEX
            | kinds::LITERAL_BOOL
            | kinds::LITERAL_STRING
            | kinds::LITERAL_NAME
    )
}

fn collect_comments(root: Node) -> Vec<Node> {
    let mut comments = Vec::new();
    let mut cursor = root.walk();
    loop {
        if cursor.node().kind() == kinds::COMMENT {
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

pub fn format_document(root: Node, source: &str, options: FormatOptions) -> String {
    let indent_unit = indent_unit_for(&options);
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
}
