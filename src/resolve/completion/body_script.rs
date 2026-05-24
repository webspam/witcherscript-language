use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;

use super::super::ast::nodes_at_offset;

const MODDING_ANNOTATIONS: &[&str] = &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"];

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
