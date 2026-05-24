use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;

use super::super::ast::nodes_at_offset;

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
