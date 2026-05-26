use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

use super::ast::{nodes_at_offset, significant_node_before_byte};
use super::definition::resolve_definition_at_byte;
use super::symbol_db::SymbolDb;
use super::Definition;

#[derive(Debug, Clone)]
pub struct SignatureHelpInfo {
    pub label: String,
    /// `[start, end)` UTF-16 offsets of each parameter substring within `label`.
    pub parameters: Vec<(u32, u32)>,
    pub active_parameter: Option<u32>,
}

/// A call site around the cursor: a closed `func_call_expr`, or an unclosed call recovered as an `ERROR` node.
struct CallSite<'tree> {
    callee: Node<'tree>,
    open_paren_byte: usize,
    args: Option<Node<'tree>>,
}

pub fn signature_help(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
    compact_colon: bool,
) -> Option<SignatureHelpInfo> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let call = locate_call(
        document.tree.root_node(),
        document.source.as_bytes(),
        byte_offset,
    )?;

    let callee_ident = match call.callee.kind() {
        "ident" => call.callee,
        "member_access_expr" | "incomplete_member_access_expr" => call
            .callee
            .child_by_field_name("member")
            .filter(|m| m.kind() == "ident")?,
        _ => return None,
    };
    let definition = resolve_definition_at_byte(uri, document, db, callee_ident.start_byte())?;
    if !matches!(
        definition.symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Event
    ) {
        return None;
    }

    let params = db.full_parameters_of(&definition.uri, definition.symbol.id);
    let colon = if compact_colon { ": " } else { " : " };

    let mut label = String::new();
    label.push_str(&definition.symbol.name);
    label.push('(');
    let mut parameters = Vec::with_capacity(params.len());
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let start = label.encode_utf16().count() as u32;
        if param.is_optional {
            label.push_str("optional ");
        }
        if param.is_out {
            label.push_str("out ");
        }
        label.push_str(&param.name);
        if let Some(ty) = &param.type_annotation {
            label.push_str(colon);
            label.push_str(ty);
        }
        let end = label.encode_utf16().count() as u32;
        parameters.push((start, end));
    }
    label.push(')');
    if let Some(ret) = &definition.symbol.type_annotation {
        if ret != "void" {
            label.push_str(colon);
            label.push_str(ret);
        }
    }

    let active_parameter = if params.is_empty() {
        None
    } else {
        let comma_count = call
            .args
            .map(|args| {
                let mut cursor = args.walk();
                args.children(&mut cursor)
                    .filter(|c| c.kind() == "," && c.start_byte() < byte_offset)
                    .count()
            })
            .unwrap_or(0);
        Some((comma_count as u32).min(params.len() as u32 - 1))
    };

    Some(SignatureHelpInfo {
        label,
        parameters,
        active_parameter,
    })
}

fn locate_call<'tree>(
    root: Node<'tree>,
    source: &[u8],
    byte_offset: usize,
) -> Option<CallSite<'tree>> {
    let mut best: Option<CallSite> = None;
    let seeds = nodes_at_offset(root, byte_offset)
        .into_iter()
        .chain(significant_node_before_byte(root, source, byte_offset));
    for start in seeds {
        let mut node = Some(start);
        while let Some(current) = node {
            if let Some(site) = call_site_of(current, byte_offset) {
                if best
                    .as_ref()
                    .is_none_or(|b| site.open_paren_byte > b.open_paren_byte)
                {
                    best = Some(site);
                }
                break;
            }
            node = current.parent();
        }
    }
    best
}

/// Interprets `node` as a call site if the cursor sits between its `(` and `)`.
fn call_site_of(node: Node, byte_offset: usize) -> Option<CallSite> {
    match node.kind() {
        "func_call_expr" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let open = children.iter().find(|c| c.kind() == "(")?;
            if open.start_byte() >= byte_offset {
                return None;
            }
            let closed_before_cursor = children
                .iter()
                .find(|c| c.kind() == ")")
                .filter(|c| !c.is_missing())
                .is_some_and(|c| c.start_byte() < byte_offset);
            if closed_before_cursor {
                return None;
            }
            Some(CallSite {
                callee: node.child_by_field_name("func")?,
                open_paren_byte: open.start_byte(),
                args: node.child_by_field_name("args"),
            })
        }
        "ERROR" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let open_idx = children
                .iter()
                .rposition(|c| c.kind() == "(" && c.start_byte() < byte_offset)?;
            let open = children[open_idx];
            let callee = children[..open_idx]
                .iter()
                .rev()
                .find(|c| c.is_named())
                .copied()?;
            let args = children
                .get(open_idx + 1)
                .filter(|c| c.kind() == "func_call_args")
                .copied();
            Some(CallSite {
                callee,
                open_paren_byte: open.start_byte(),
                args,
            })
        }
        _ => None,
    }
}

pub fn hover_text(definition: &Definition) -> String {
    let symbol = &definition.symbol;
    let mut lines = Vec::new();

    if !symbol.annotations.is_empty() {
        let annotations = symbol
            .annotations
            .iter()
            .map(|annotation| match &annotation.argument {
                Some(argument) => format!("@{}({argument})", annotation.name),
                None => format!("@{}", annotation.name),
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(annotations);
    }

    match symbol.kind {
        SymbolKind::Method => {
            let params_and_return = symbol.signature.as_deref().unwrap_or("");
            let class_prefix = symbol
                .container_name
                .as_deref()
                .map(|cn| format!("{cn}."))
                .unwrap_or_default();
            lines.push(format!(
                "(method) {}{}{}",
                class_prefix, symbol.name, params_and_return
            ));
        }
        SymbolKind::Field => {
            if let Some(sig) = &symbol.signature {
                lines.push(format!("(field) {sig}"));
            } else if let Some(type_annotation) = &symbol.type_annotation {
                lines.push(format!("(field) {} : {type_annotation}", symbol.name));
            } else {
                lines.push(format!("(field) {}", symbol.name));
            }
        }
        _ => {
            let label = match symbol.kind {
                SymbolKind::Class => "class",
                SymbolKind::Struct => "struct",
                SymbolKind::Enum => "enum",
                SymbolKind::EnumMember => "enum member",
                SymbolKind::Function => "function",
                SymbolKind::Variable => "var",
                SymbolKind::Parameter => "(parameter)",
                SymbolKind::State => "state",
                SymbolKind::Event => "event",
                SymbolKind::Method | SymbolKind::Field => unreachable!(),
            };
            if let Some(sig) = &symbol.signature {
                let flavour_prefix = symbol
                    .flavour
                    .as_deref()
                    .map(|f| format!("{f} "))
                    .unwrap_or_default();
                lines.push(format!("{flavour_prefix}{label} {}{sig}", symbol.name));
            } else if let Some(type_annotation) = &symbol.type_annotation {
                lines.push(format!("{label} {} : {type_annotation}", symbol.name));
            } else {
                lines.push(format!("{label} {}", symbol.name));
            }
            if let Some(detail) = symbol.display_detail() {
                match lines.last_mut() {
                    Some(last) => {
                        last.push(' ');
                        last.push_str(&detail);
                    }
                    None => lines.push(detail),
                }
            }
        }
    }

    lines.join("\n")
}
