use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{AccessLevel, SymbolKind};

use super::super::ast::{
    find_ancestor_of_kind, is_kind_or_error_wrapped_kind, is_statement_boundary,
    nearest_enclosing_block, nodes_at_offset, significant_node_before_byte,
};
use super::super::db::SymbolDb;
use super::super::inference::enclosing_type_context;
use super::super::Definition;

const MODDING_ANNOTATIONS: &[&str] = &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"];

pub struct StatementCompletions {
    pub locals: Vec<Definition>,
    pub members: Vec<Definition>,
    pub globals: Vec<Definition>,
    pub has_this: bool,
    pub has_super: bool,
    pub in_switch: bool,
    pub in_loop: bool,
}

pub fn statement_completions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> StatementCompletions {
    statement_completions_inner(uri, document, db, position).unwrap_or(StatementCompletions {
        locals: vec![],
        members: vec![],
        globals: vec![],
        has_this: false,
        has_super: false,
        in_switch: false,
        in_loop: false,
    })
}

fn statement_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<StatementCompletions> {
    const STMT_WRITER_KINDS: &[&str] = &[
        "ident", "var", "this", "super", "if", "else", "do", "while", "for", "switch", "return",
        "case", "default",
    ];
    let (nodes, base) = function_body_completions(
        uri,
        document,
        db,
        position,
        is_statement_boundary,
        STMT_WRITER_KINDS,
    )?;

    let in_switch = nodes
        .iter()
        .any(|n| nearest_enclosing_block(*n).is_some_and(|b| b.kind() == "switch_block"));

    let in_loop = nodes
        .iter()
        .any(|n| find_ancestor_of_kind(*n, &["for_stmt", "while_stmt", "do_while_stmt"]).is_some());

    Some(StatementCompletions {
        locals: base.locals,
        members: base.members,
        globals: base.globals,
        has_this: base.has_this,
        has_super: base.has_super,
        in_switch,
        in_loop,
    })
}

pub struct ExpressionCompletions {
    pub locals: Vec<Definition>,
    pub members: Vec<Definition>,
    pub globals: Vec<Definition>,
    pub has_this: bool,
    pub has_super: bool,
}

pub fn expression_completions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<ExpressionCompletions> {
    expression_completions_inner(uri, document, db, position)
}

fn is_expression_boundary(node: Node) -> bool {
    matches!(
        node.kind(),
        "(" | ","
            | "="
            | "return"
            | "assign_op_direct"
            | "assign_op_sum"
            | "assign_op_diff"
            | "assign_op_mult"
            | "assign_op_div"
            | "assign_op_bitand"
            | "assign_op_bitor"
            | "binary_op_or"
            | "binary_op_and"
            | "binary_op_bitor"
            | "binary_op_bitand"
            | "binary_op_bitxor"
            | "binary_op_eq"
            | "binary_op_neq"
            | "binary_op_gt"
            | "binary_op_ge"
            | "binary_op_lt"
            | "binary_op_le"
            | "binary_op_diff"
            | "binary_op_sum"
            | "binary_op_mod"
            | "binary_op_div"
            | "binary_op_mult"
            | "unary_op_neg"
            | "unary_op_not"
            | "unary_op_bitnot"
            | "unary_op_plus"
    )
}

fn expression_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<ExpressionCompletions> {
    let (_, base) = function_body_completions(
        uri,
        document,
        db,
        position,
        is_expression_boundary,
        &["ident"],
    )?;

    Some(ExpressionCompletions {
        locals: base.locals,
        members: base.members,
        globals: base.globals,
        has_this: base.has_this,
        has_super: base.has_super,
    })
}

struct FunctionBodyContext {
    locals: Vec<Definition>,
    members: Vec<Definition>,
    globals: Vec<Definition>,
    has_this: bool,
    has_super: bool,
}

fn function_body_completions<'a>(
    uri: &str,
    document: &'a ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
    boundary: fn(Node) -> bool,
    writer_kinds: &[&str],
) -> Option<(Vec<Node<'a>>, FunctionBodyContext)> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);

    let prev = significant_node_before_byte(root, document.source.as_bytes(), byte_offset);
    let at_start = prev.is_some_and(boundary);
    let writing_first = nodes
        .last()
        .filter(|&n| is_kind_or_error_wrapped_kind(*n, writer_kinds))
        .and_then(|n| {
            significant_node_before_byte(root, document.source.as_bytes(), n.start_byte())
        })
        .is_some_and(boundary);
    if !at_start && !writing_first {
        return None;
    }

    if !nodes
        .iter()
        .any(|n| find_ancestor_of_kind(*n, &["func_block"]).is_some())
    {
        return None;
    }

    let callable = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    )?;

    let locals: Vec<Definition> = document
        .symbols
        .children_of(Some(callable.id))
        .filter(|sym| {
            matches!(sym.kind, SymbolKind::Variable | SymbolKind::Parameter)
                && sym.selection_byte_range.start <= byte_offset
        })
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
        .collect();

    let current_type = enclosing_type_context(document, db, byte_offset);
    let members: Vec<Definition> = current_type
        .as_ref()
        .map(|t| db.members_of(&t.name, AccessLevel::Private))
        .unwrap_or_default();
    let has_this = current_type.is_some();
    let has_super = current_type
        .as_ref()
        .and_then(|t| t.base_class.as_deref())
        .is_some();

    let mut globals = db.all_top_level_callables();
    globals.extend(db.all_script_globals());
    globals.extend(db.all_enum_variants());

    Some((
        nodes,
        FunctionBodyContext {
            locals,
            members,
            globals,
            has_this,
            has_super,
        },
    ))
}

#[derive(Clone, Copy, PartialEq)]
enum ClassBodyKind {
    Class,
    State,
    Struct,
}

struct ClassBodyCtx {
    kind: ClassBodyKind,
    has_import: bool,
    has_access: bool,
    has_final: bool,
    has_latent: bool,
    has_editable: bool,
    has_saved: bool,
    has_const_: bool,
    has_inlined: bool,
    has_optional: bool,
    saw_decl_keyword: bool,
}

impl ClassBodyCtx {
    fn has_any(&self) -> bool {
        self.has_import
            || self.has_access
            || self.has_final
            || self.has_latent
            || self.has_editable
            || self.has_saved
            || self.has_const_
            || self.has_inlined
            || self.has_optional
    }
}

pub fn class_body_keyword_completions(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Vec<&'static str> {
    class_body_kw_inner(document, position).unwrap_or_default()
}

fn class_body_kw_inner(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<Vec<&'static str>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);

    let kind = nodes.iter().find_map(|n| enclosing_body_kind(*n))?;
    let class_body = nodes.iter().find_map(|n| enclosing_class_body_node(*n))?;

    let mut ctx = ClassBodyCtx {
        kind,
        has_import: false,
        has_access: false,
        has_final: false,
        has_latent: false,
        has_editable: false,
        has_saved: false,
        has_const_: false,
        has_inlined: false,
        has_optional: false,
        saw_decl_keyword: false,
    };

    if let Some(child) = class_body_child_at_cursor(class_body, byte_offset) {
        let cursor_inside = byte_offset < child.end_byte();
        if cursor_inside || child.is_error() {
            let limit = if cursor_inside {
                byte_offset
            } else {
                child.end_byte()
            };
            let mut cur = child.walk();
            for ch in child.children(&mut cur) {
                if ch.start_byte() >= limit {
                    break;
                }
                if ch.kind() == "specifier" {
                    match ch.utf8_text(document.source.as_bytes()).unwrap_or("") {
                        "private" | "protected" | "public" => ctx.has_access = true,
                        "import" => ctx.has_import = true,
                        "final" => ctx.has_final = true,
                        "latent" => ctx.has_latent = true,
                        "editable" => ctx.has_editable = true,
                        "saved" => ctx.has_saved = true,
                        "const" => ctx.has_const_ = true,
                        "inlined" => ctx.has_inlined = true,
                        "optional" => ctx.has_optional = true,
                        _ => {}
                    }
                } else if matches!(
                    ch.kind(),
                    "var" | "function" | "event" | "autobind" | "default" | "defaults" | "hint"
                ) {
                    ctx.saw_decl_keyword = true;
                    break;
                }
                // unknown token (partial ident etc.) — ignore, don't affect context
            }
        }
        // cursor after a complete declaration: ctx stays empty, offer all keywords
    }

    if ctx.saw_decl_keyword {
        return None;
    }

    Some(class_body_kw_candidates(&ctx))
}

fn enclosing_class_body_node(mut node: Node) -> Option<Node> {
    loop {
        match node.kind() {
            "func_block" | "member_default_val_block" | "script" => return None,
            "class_def" | "struct_def" => return Some(node),
            _ => node = node.parent()?,
        }
    }
}

fn class_body_child_at_cursor(class_body: Node, byte_offset: usize) -> Option<Node> {
    let mut cur = class_body.walk();
    let mut result = None;
    for child in class_body.children(&mut cur) {
        if !child.is_named() {
            continue;
        }
        if child.start_byte() > byte_offset {
            break;
        }
        result = Some(child);
    }
    result
}

fn enclosing_body_kind(mut node: Node) -> Option<ClassBodyKind> {
    loop {
        match node.kind() {
            "func_block" | "member_default_val_block" => return None,
            "script" => return None,
            "class_def" => {
                return node.parent().and_then(|p| match p.kind() {
                    "class_decl" => Some(ClassBodyKind::Class),
                    "state_decl" => Some(ClassBodyKind::State),
                    _ => None,
                });
            }
            "struct_def" => return Some(ClassBodyKind::Struct),
            _ => {}
        }
        match node.parent() {
            Some(p) => node = p,
            None => return None,
        }
    }
}

fn class_body_kw_candidates(ctx: &ClassBodyCtx) -> Vec<&'static str> {
    let mut kw: Vec<&'static str> = Vec::new();

    if !ctx.has_any() {
        kw.extend_from_slice(&["private", "protected", "public", "import"]);
        kw.extend_from_slice(&["editable", "saved", "const", "inlined"]);
        if ctx.kind != ClassBodyKind::Struct {
            kw.extend_from_slice(&["final", "latent", "optional"]);
        }
        kw.push("var");
        if ctx.kind != ClassBodyKind::Struct {
            kw.extend_from_slice(&["function", "event", "autobind"]);
        }
        kw.extend_from_slice(&["default", "defaults", "hint"]);
        return kw;
    }

    // Access must be the first specifier (after import). Once any other
    // specifier has been typed, access modifiers can no longer be added.
    let non_access_seen = ctx.has_final
        || ctx.has_latent
        || ctx.has_editable
        || ctx.has_saved
        || ctx.has_const_
        || ctx.has_inlined
        || ctx.has_optional;
    if !ctx.has_access && !non_access_seen {
        kw.extend_from_slice(&["private", "protected", "public"]);
    }

    let in_var_path = ctx.has_editable || ctx.has_saved || ctx.has_const_ || ctx.has_inlined;
    let in_func_path = ctx.has_final || ctx.has_latent;
    let in_autobind_path = ctx.has_optional;

    if ctx.kind != ClassBodyKind::Struct && !in_var_path && !in_autobind_path {
        if !ctx.has_final {
            kw.push("final");
        }
        if !ctx.has_latent {
            kw.push("latent");
        }
    }

    if !ctx.has_import && !in_func_path && !in_autobind_path {
        // saved and inlined are terminal — nothing can follow them.
        // Valid non-trivial sequences: editable→{saved|inlined}, const→inlined.
        let var_path_done = ctx.has_saved || ctx.has_inlined;
        if !var_path_done {
            if !ctx.has_editable && !ctx.has_const_ && !ctx.has_saved {
                kw.extend_from_slice(&["editable", "saved", "const", "inlined"]);
            } else if ctx.has_editable && !ctx.has_saved && !ctx.has_const_ {
                // editable can be followed by saved or inlined (not const)
                kw.extend_from_slice(&["saved", "inlined"]);
            } else if ctx.has_const_ {
                // const can only be followed by inlined
                kw.push("inlined");
            }
            // saved alone: terminal — no more var specifiers
        }
    }

    if ctx.kind != ClassBodyKind::Struct
        && !ctx.has_optional
        && !ctx.has_import
        && !in_var_path
        && !in_func_path
    {
        kw.push("optional");
    }

    let can_var = !in_func_path && !in_autobind_path;
    let can_function = ctx.kind != ClassBodyKind::Struct && !in_var_path && !in_autobind_path;
    let can_autobind =
        ctx.kind != ClassBodyKind::Struct && !in_var_path && !in_func_path && !ctx.has_import;

    if can_var {
        kw.push("var");
    }
    if can_function {
        kw.push("function");
    }
    if can_autobind {
        kw.push("autobind");
    }

    kw
}

#[derive(Clone, Copy)]
enum MemberAnnotation {
    Field,
    Method,
}

#[derive(Default)]
struct ScriptBodyCtx {
    has_import: bool,
    has_statemachine: bool,
    has_abstract: bool,
    has_final: bool,
    has_latent: bool,
    has_flavour: bool,
    member_annotation: Option<MemberAnnotation>,
    saw_decl_keyword: bool,
}

impl ScriptBodyCtx {
    fn has_any(&self) -> bool {
        self.has_import
            || self.has_statemachine
            || self.has_abstract
            || self.has_final
            || self.has_latent
            || self.has_flavour
    }
}

pub fn script_body_completions(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Vec<&'static str> {
    script_body_inner(document, position).unwrap_or_default()
}

fn script_body_inner(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<Vec<&'static str>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);

    nodes.iter().find_map(|n| enclosing_script_scope(*n))?;

    let mut ctx = ScriptBodyCtx::default();

    if let Some(child) = script_child_at_cursor(root, byte_offset) {
        let cursor_inside = byte_offset < child.end_byte();
        if cursor_inside || child.is_error() {
            let limit = if cursor_inside {
                byte_offset
            } else {
                child.end_byte()
            };
            collect_script_ctx(child, document.source.as_bytes(), limit, &mut ctx);
        }
    }

    if ctx.saw_decl_keyword {
        return None;
    }

    Some(script_body_candidates(&ctx))
}

fn collect_script_ctx(node: Node, source: &[u8], limit: usize, ctx: &mut ScriptBodyCtx) {
    let mut cur = node.walk();
    for ch in node.children(&mut cur) {
        if ch.start_byte() >= limit {
            break;
        }
        match ch.kind() {
            "specifier" => match ch.utf8_text(source).unwrap_or("") {
                "import" => ctx.has_import = true,
                "statemachine" => ctx.has_statemachine = true,
                "abstract" => ctx.has_abstract = true,
                "final" => ctx.has_final = true,
                "latent" => ctx.has_latent = true,
                _ => {}
            },
            "func_flavour" => ctx.has_flavour = true,
            "cleanup" | "entry" | "exec" | "quest" | "reward" | "storyscene" | "timer" => {
                ctx.has_flavour = true;
            }
            // @addField/@addMethod inject a class member — member specifiers follow.
            "annotation" => {
                let name = ch
                    .children(&mut ch.walk())
                    .find(|c| c.kind() == "annotation_ident")
                    .and_then(|n| n.utf8_text(source).ok());
                match name {
                    Some("@addField") => ctx.member_annotation = Some(MemberAnnotation::Field),
                    Some("@addMethod") => ctx.member_annotation = Some(MemberAnnotation::Method),
                    _ => {}
                }
            }
            "class" | "state" | "struct" | "enum" | "function" | "var" => {
                ctx.saw_decl_keyword = true;
                return;
            }
            "ERROR" => collect_script_ctx(ch, source, limit, ctx),
            _ => {}
        }
    }
}

fn enclosing_script_scope(mut node: Node) -> Option<Node> {
    loop {
        match node.kind() {
            "func_block"
            | "class_def"
            | "struct_def"
            | "member_default_val_block"
            | "switch_block" => return None,
            "script" => return Some(node),
            _ => {}
        }
        node = node.parent()?;
    }
}

fn script_child_at_cursor(script: Node, byte_offset: usize) -> Option<Node> {
    let mut cur = script.walk();
    let mut result = None;
    for child in script.children(&mut cur) {
        if !child.is_named() {
            continue;
        }
        if child.start_byte() > byte_offset {
            break;
        }
        result = Some(child);
    }
    result
}

fn script_body_candidates(ctx: &ScriptBodyCtx) -> Vec<&'static str> {
    if let Some(member) = ctx.member_annotation {
        return member_annotation_candidates(member, ctx);
    }

    let mut kw: Vec<&'static str> = Vec::new();

    let in_func_path = ctx.has_final || ctx.has_latent || ctx.has_flavour;

    let can_class = !in_func_path;
    let can_state = can_class && !ctx.has_statemachine;
    let can_struct = can_state && !ctx.has_abstract;
    let can_enum = !ctx.has_any();
    let can_function = !ctx.has_statemachine && !ctx.has_abstract;
    let can_var = !ctx.has_any();

    if !ctx.has_import && !in_func_path {
        kw.push("import");
    }
    if !ctx.has_statemachine && !in_func_path && !ctx.has_abstract {
        kw.push("statemachine");
    }
    if !ctx.has_abstract && !in_func_path {
        kw.push("abstract");
    }
    if !ctx.has_final && can_function && !ctx.has_latent && !ctx.has_flavour {
        kw.push("final");
    }
    if !ctx.has_latent && can_function && !ctx.has_flavour {
        kw.push("latent");
    }
    if !ctx.has_flavour && can_function {
        kw.extend_from_slice(&[
            "cleanup",
            "entry",
            "exec",
            "quest",
            "reward",
            "storyscene",
            "timer",
        ]);
    }

    if can_class {
        kw.push("class");
    }
    if can_state {
        kw.push("state");
    }
    if can_struct {
        kw.push("struct");
    }
    if can_enum {
        kw.push("enum");
    }
    if can_function {
        kw.push("function");
    }
    if can_var {
        kw.push("var");
    }

    if !ctx.has_any() {
        kw.extend_from_slice(MODDING_ANNOTATIONS);
    }

    kw
}

fn member_annotation_candidates(
    member: MemberAnnotation,
    ctx: &ScriptBodyCtx,
) -> Vec<&'static str> {
    let mut kw: Vec<&'static str> = Vec::new();

    if !ctx.has_any() {
        kw.extend_from_slice(&["private", "protected", "public"]);
    }

    match member {
        MemberAnnotation::Field => {
            kw.extend_from_slice(&["editable", "saved", "const", "inlined", "var"]);
        }
        MemberAnnotation::Method => {
            if !ctx.has_final {
                kw.push("final");
            }
            if !ctx.has_latent {
                kw.push("latent");
            }
            kw.extend_from_slice(&["function", "event"]);
        }
    }

    kw
}
