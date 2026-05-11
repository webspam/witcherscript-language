use std::collections::HashMap;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;
use crate::symbols::{DocumentSymbols, Symbol, SymbolId, SymbolKind};

#[derive(Debug, Clone)]
pub struct Definition {
    pub uri: String,
    pub symbol: Symbol,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    documents: HashMap<String, Vec<Symbol>>,
}

impl WorkspaceIndex {
    pub fn update_document(&mut self, uri: impl Into<String>, symbols: &DocumentSymbols) {
        self.documents.insert(uri.into(), symbols.all().to_vec());
    }

    pub fn remove_document(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    pub fn find_top_level(&self, name: &str) -> Option<Definition> {
        self.documents.iter().find_map(|(uri, symbols)| {
            symbols
                .iter()
                .find(|symbol| symbol.container.is_none() && symbol.name == name)
                .cloned()
                .map(|symbol| Definition {
                    uri: uri.clone(),
                    symbol,
                })
        })
    }

    pub fn find_member(&self, container_name: &str, name: &str) -> Option<Definition> {
        self.documents.iter().find_map(|(uri, symbols)| {
            let container = symbols
                .iter()
                .find(|symbol| symbol.name == container_name && is_type_like(symbol.kind))?;
            symbols
                .iter()
                .find(|symbol| symbol.container == Some(container.id) && symbol.name == name)
                .cloned()
                .map(|symbol| Definition {
                    uri: uri.clone(),
                    symbol,
                })
        })
    }
}

pub fn resolve_definition(
    uri: &str,
    document: &ParsedDocument,
    workspace: &WorkspaceIndex,
    position: SourcePosition,
) -> Option<Definition> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let ident = identifier_at(document.tree.root_node(), byte_offset)?;
    let name = ident.utf8_text(document.source.as_bytes()).ok()?;

    if let Some(member_definition) = resolve_member_access(uri, document, workspace, ident, name) {
        return Some(member_definition);
    }

    resolve_local_or_parameter(uri, document, byte_offset, name)
        .or_else(|| resolve_current_type_member(uri, document, byte_offset, name))
        .or_else(|| resolve_document_top_level(uri, document, name))
        .or_else(|| workspace.find_top_level(name))
}

pub fn hover_text(definition: &Definition) -> String {
    let symbol = &definition.symbol;
    let mut lines = Vec::new();
    let label = match symbol.kind {
        SymbolKind::Class => "class",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::EnumVariant => "enum variant",
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Field => "field",
        SymbolKind::Variable => "local",
        SymbolKind::Parameter => "parameter",
        SymbolKind::State => "state",
        SymbolKind::Event => "event",
    };

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

    if let Some(signature) = &symbol.signature {
        lines.push(signature.clone());
    } else if let Some(type_annotation) = &symbol.type_annotation {
        lines.push(format!("{label} {} : {type_annotation}", symbol.name));
    } else {
        lines.push(format!("{label} {}", symbol.name));
    }

    if let Some(detail) = &symbol.detail {
        lines.push(detail.clone());
    }

    lines.join("\n")
}

fn resolve_member_access(
    uri: &str,
    document: &ParsedDocument,
    workspace: &WorkspaceIndex,
    ident: Node,
    name: &str,
) -> Option<Definition> {
    let parent = ident.parent()?;
    if parent.kind() != "member_access_expr" {
        return None;
    }

    let receiver = first_named_child(parent)?;
    match receiver.kind() {
        "this_expr" => {
            let current_type = current_type_name(document, ident.start_byte())?;
            resolve_document_member(uri, document, &current_type, name)
                .or_else(|| workspace.find_member(&current_type, name))
        }
        "parent_expr" | "super_expr" => {
            let current_type = current_type_symbol(document, ident.start_byte())?;
            let base_name = current_type.detail.as_deref()?.strip_prefix("extends ")?;
            workspace.find_member(base_name, name)
        }
        "ident" => {
            let receiver_name = receiver.utf8_text(document.source.as_bytes()).ok()?;
            resolve_document_member(uri, document, receiver_name, name)
                .or_else(|| workspace.find_member(receiver_name, name))
        }
        _ => None,
    }
}

fn resolve_local_or_parameter(
    uri: &str,
    document: &ParsedDocument,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    let function = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    )?;
    document
        .symbols
        .children_of(Some(function.id))
        .filter(|symbol| {
            matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Parameter)
                && symbol.name == name
                && symbol.selection_byte_range.start <= byte_offset
        })
        .max_by_key(|symbol| symbol.selection_byte_range.start)
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
}

fn resolve_current_type_member(
    uri: &str,
    document: &ParsedDocument,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    let current_type = current_type_name(document, byte_offset)?;
    resolve_document_member(uri, document, &current_type, name)
}

fn resolve_document_member(
    uri: &str,
    document: &ParsedDocument,
    container_name: &str,
    name: &str,
) -> Option<Definition> {
    let container = document
        .symbols
        .all()
        .iter()
        .find(|symbol| symbol.name == container_name && is_type_like(symbol.kind))?;
    document
        .symbols
        .children_of(Some(container.id))
        .find(|symbol| symbol.name == name)
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
}

fn resolve_document_top_level(
    uri: &str,
    document: &ParsedDocument,
    name: &str,
) -> Option<Definition> {
    document
        .symbols
        .children_of(None)
        .find(|symbol| symbol.name == name)
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
}

fn current_type_name(document: &ParsedDocument, byte_offset: usize) -> Option<String> {
    current_type_symbol(document, byte_offset).map(|symbol| symbol.name.clone())
}

fn current_type_symbol(document: &ParsedDocument, byte_offset: usize) -> Option<&Symbol> {
    document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
    )
}

fn identifier_at(root: Node, byte_offset: usize) -> Option<Node> {
    let node = root.descendant_for_byte_range(byte_offset, byte_offset)?;
    if node.kind() == "ident" {
        return Some(node);
    }

    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "ident" {
            return Some(parent);
        }
        current = parent;
    }

    None
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let child = node.named_children(&mut cursor).next();
    child
}

fn is_type_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
    )
}

#[allow(dead_code)]
fn symbol_id(symbol: &Symbol) -> SymbolId {
    symbol.id
}

#[cfg(test)]
mod tests {
    use crate::document::parse_document;
    use crate::line_index::SourcePosition;

    use super::{resolve_definition, WorkspaceIndex};

    #[test]
    fn resolves_parameter_before_top_level() {
        let document =
            parse_document("function value() {}\nfunction test(value : int) {\n value = 1;\n}\n")
                .expect("parse should succeed");
        let mut index = WorkspaceIndex::default();
        index.update_document("file:///test.ws", &document.symbols);

        let definition = resolve_definition(
            "file:///test.ws",
            &document,
            &index,
            SourcePosition {
                line: 2,
                character: 1,
            },
        )
        .expect("definition should resolve");

        assert_eq!(
            definition.symbol.kind,
            crate::symbols::SymbolKind::Parameter
        );
    }
}
