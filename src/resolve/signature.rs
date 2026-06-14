use tree_sitter::Node;

use crate::cst::grammar::callee_ident;
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{Symbol, SymbolKind};
use crate::types::Type;

use super::Definition;
use super::ast::{nodes_at_offset, significant_node_before_byte};
use super::definition::resolve_definition_at_byte;
use super::symbol_db::SymbolDb;

#[derive(Debug, Clone)]
pub struct SignatureHelpInfo {
    pub label: String,
    /// `[start, end)` UTF-16 offsets of each parameter substring within `label`.
    pub parameters: Vec<(usize, usize)>,
    pub active_parameter: Option<usize>,
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

    let ident = callee_ident(call.callee)?;
    let definition = resolve_definition_at_byte(uri, document, db, ident.start_byte())?;
    if !definition.symbol.kind.is_callable() {
        return None;
    }

    let params = db.display_parameters_of(&definition);
    let colon = if compact_colon { ": " } else { " : " };

    let mut label = String::new();
    label.push_str(&definition.symbol.name);
    label.push('(');
    let mut parameters = Vec::with_capacity(params.len());
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let start = label.encode_utf16().count();
        push_parameter(&mut label, param, colon);
        let end = label.encode_utf16().count();
        parameters.push((start, end));
    }
    label.push(')');
    if let Some(ret) = &definition.symbol.type_annotation
        && *ret != Type::Void
    {
        label.push_str(colon);
        label.push_str(&ret.to_string());
    }

    let active_parameter = if params.is_empty() {
        None
    } else {
        let comma_count = call.args.map_or(0, |args| {
            let mut cursor = args.walk();
            args.children(&mut cursor)
                .filter(|c| c.kind() == "," && c.start_byte() < byte_offset)
                .count()
        });
        Some(comma_count.min(params.len() - 1))
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
        kinds::FUNC_CALL_EXPR => {
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
                callee: node.child_by_field_name(fields::FUNC)?,
                open_paren_byte: open.start_byte(),
                args: node.child_by_field_name(fields::ARGS),
            })
        }
        kinds::ERROR => {
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
                .filter(|c| c.kind() == kinds::FUNC_CALL_ARGS)
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

fn push_parameter(label: &mut String, param: &Symbol, colon: &str) {
    if param.specifiers.is_optional() {
        label.push_str("optional ");
    }
    if param.specifiers.is_out() {
        label.push_str("out ");
    }
    label.push_str(&param.name);
    if let Some(ty) = &param.type_annotation {
        label.push_str(colon);
        label.push_str(&ty.to_string());
    }
}

/// `colon` varies by context: `": "` for compact hover, `" : "` for inserted code.
pub fn render_parameters(params: &[Symbol], colon: &str) -> String {
    let mut out = String::from("(");
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        push_parameter(&mut out, param, colon);
    }
    out.push(')');
    out
}

pub fn render_signature(params: &[Symbol], return_type: Option<&Type>, colon: &str) -> String {
    let mut out = render_parameters(params, colon);
    if let Some(ret) = return_type {
        out.push_str(colon);
        out.push_str(&ret.to_string());
    }
    out
}

/// `access specifier* flavour?` in canonical order; ends with a trailing space when non-empty.
fn declaration_keywords(symbol: &Symbol) -> String {
    let mut out = String::new();
    if let Some(access) = symbol.access.as_keyword() {
        out.push_str(access);
        out.push(' ');
    }
    for specifier in symbol.specifiers.iter() {
        out.push_str(specifier.as_keyword());
        out.push(' ');
    }
    if let Some(flavour) = symbol.flavour {
        out.push_str(flavour.as_keyword());
        out.push(' ');
    }
    out
}

pub fn hover_text(definition: &Definition, db: &SymbolDb, compact_colon: bool) -> String {
    let symbol = &definition.symbol;
    let colon = if compact_colon { ": " } else { " : " };
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
            let params_and_return = render_signature(
                &db.display_parameters_of(definition),
                symbol.type_annotation.as_ref(),
                colon,
            );
            let class_prefix = symbol
                .container_name
                .as_deref()
                .map(|cn| format!("{cn}."))
                .unwrap_or_default();
            lines.push(format!(
                "(method) {}{}{}{}",
                declaration_keywords(symbol),
                class_prefix,
                symbol.name,
                params_and_return
            ));
        }
        SymbolKind::Field => {
            let keywords = declaration_keywords(symbol);
            match &symbol.type_annotation {
                Some(type_annotation) => {
                    lines.push(format!(
                        "(field) {keywords}{}{colon}{type_annotation}",
                        symbol.name
                    ));
                }
                None => lines.push(format!("(field) {keywords}{}", symbol.name)),
            }
        }
        _ => {
            let label = match symbol.kind {
                SymbolKind::Class => "class",
                SymbolKind::NativeType => "native type",
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
            if symbol.kind.is_callable() {
                let sig = render_signature(
                    &db.display_parameters_of(definition),
                    symbol.type_annotation.as_ref(),
                    colon,
                );
                let keywords = declaration_keywords(symbol);
                lines.push(format!("{keywords}{label} {}{sig}", symbol.name));
            } else if let Some(type_annotation) = &symbol.type_annotation {
                lines.push(format!("{label} {}{colon}{type_annotation}", symbol.name));
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
