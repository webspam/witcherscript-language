use std::collections::HashMap;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::{SourcePosition, SourceRange};
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
        .or_else(|| resolve_at_definition_site(uri, document, byte_offset, name))
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
        SymbolKind::Variable => "var",
        SymbolKind::Parameter => "(parameter)",
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
    for offset in [byte_offset, byte_offset.saturating_sub(1)] {
        let node = root.descendant_for_byte_range(offset, offset)?;
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
        if offset == 0 {
            break;
        }
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

pub fn find_references(
    definition: &Definition,
    definition_document: &ParsedDocument,
    search_documents: &[(&str, &ParsedDocument)],
    include_declaration: bool,
) -> Vec<(String, SourceRange)> {
    let name = &definition.symbol.name;

    let scope: Option<std::ops::Range<usize>> = if matches!(
        definition.symbol.kind,
        SymbolKind::Variable | SymbolKind::Parameter
    ) {
        definition
            .symbol
            .container
            .and_then(|id| definition_document.symbols.by_id(id))
            .map(|container| container.byte_range.clone())
    } else {
        None
    };

    let mut results = Vec::new();

    for (uri, document) in search_documents {
        if scope.is_some() && *uri != definition.uri.as_str() {
            continue;
        }

        let mut byte_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        collect_ident_occurrences(
            document.tree.root_node(),
            document.source.as_bytes(),
            name,
            scope.as_ref(),
            &mut byte_ranges,
        );

        for byte_range in byte_ranges {
            if !include_declaration
                && *uri == definition.uri.as_str()
                && byte_range == definition.symbol.selection_byte_range
            {
                continue;
            }
            let range = document.line_index.byte_range_to_range(
                &document.source,
                byte_range.start,
                byte_range.end,
            );
            results.push((uri.to_string(), range));
        }
    }

    results
}

fn collect_ident_occurrences<'tree>(
    node: Node<'tree>,
    source: &[u8],
    name: &str,
    scope: Option<&std::ops::Range<usize>>,
    results: &mut Vec<std::ops::Range<usize>>,
) {
    if let Some(s) = scope {
        if node.end_byte() <= s.start || node.start_byte() >= s.end {
            return;
        }
    }
    if node.kind() == "ident" && node.utf8_text(source).ok() == Some(name) {
        results.push(node.start_byte()..node.end_byte());
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ident_occurrences(child, source, name, scope, results);
    }
}

fn resolve_at_definition_site(
    uri: &str,
    document: &ParsedDocument,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    document
        .symbols
        .all()
        .iter()
        .find(|symbol| {
            symbol.name == name
                && symbol.selection_byte_range.start <= byte_offset
                && byte_offset < symbol.selection_byte_range.end
        })
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
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
    fn resolves_definition_site_of_top_level_function() {
        let document = parse_document("function Foo() {}\n").expect("parse should succeed");
        let index = WorkspaceIndex::default();

        let definition = resolve_definition(
            "file:///test.ws",
            &document,
            &index,
            SourcePosition {
                line: 0,
                character: 9,
            },
        )
        .expect("definition should resolve from its own definition site");

        assert_eq!(definition.symbol.name, "Foo");
        assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Function);
    }

    #[test]
    fn resolves_definition_at_word_boundary() {
        // "function Foo() {}\n"
        //           0123
        // character 12 is just past the final 'o' of "Foo"
        let document = parse_document("function Foo() {}\n").expect("parse should succeed");
        let index = WorkspaceIndex::default();

        let definition = resolve_definition(
            "file:///test.ws",
            &document,
            &index,
            SourcePosition {
                line: 0,
                character: 12,
            },
        )
        .expect("definition should resolve when caret is one past the last letter");

        assert_eq!(definition.symbol.name, "Foo");
    }

    #[test]
    fn resolves_definition_site_of_class_method() {
        let document = parse_document("class CExample {\n function Bar() {}\n}\n")
            .expect("parse should succeed");
        let index = WorkspaceIndex::default();

        let definition = resolve_definition(
            "file:///test.ws",
            &document,
            &index,
            SourcePosition {
                line: 1,
                character: 10,
            },
        )
        .expect("definition should resolve from its own definition site");

        assert_eq!(definition.symbol.name, "Bar");
        assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Method);
    }

    #[test]
    fn resolves_definition_site_of_enum_variant() {
        let document =
            parse_document("enum EFoo {\n VALUE_A = 0\n}\n").expect("parse should succeed");
        let index = WorkspaceIndex::default();

        let definition = resolve_definition(
            "file:///test.ws",
            &document,
            &index,
            SourcePosition {
                line: 1,
                character: 1,
            },
        )
        .expect("definition should resolve from enum variant definition site");

        assert_eq!(definition.symbol.name, "VALUE_A");
        assert_eq!(
            definition.symbol.kind,
            crate::symbols::SymbolKind::EnumVariant
        );
    }

    #[test]
    fn finds_references_to_top_level_function() {
        let source = "function Foo() {}\nfunction Bar() {\n Foo();\n Foo();\n}\n";
        let document = parse_document(source).expect("parse should succeed");
        let definition = resolve_definition(
            "file:///test.ws",
            &document,
            &WorkspaceIndex::default(),
            SourcePosition {
                line: 0,
                character: 9,
            },
        )
        .expect("definition should resolve");

        let refs = super::find_references(
            &definition,
            &document,
            &[("file:///test.ws", &document)],
            false,
        );
        assert_eq!(refs.len(), 2, "two call sites expected");
    }

    #[test]
    fn find_references_respects_include_declaration() {
        let source = "function Foo() {}\nfunction Bar() {\n Foo();\n}\n";
        let document = parse_document(source).expect("parse should succeed");
        let definition = resolve_definition(
            "file:///test.ws",
            &document,
            &WorkspaceIndex::default(),
            SourcePosition {
                line: 0,
                character: 9,
            },
        )
        .expect("definition should resolve");

        let with_decl = super::find_references(
            &definition,
            &document,
            &[("file:///test.ws", &document)],
            true,
        );
        let without_decl = super::find_references(
            &definition,
            &document,
            &[("file:///test.ws", &document)],
            false,
        );
        assert_eq!(with_decl.len(), 2);
        assert_eq!(without_decl.len(), 1);
    }

    #[test]
    fn finds_references_to_local_variable_within_function_scope() {
        let source =
            "function Outer() {\n var x : int;\n x = 1;\n}\nfunction Other() {\n var x : int;\n}\n";
        let document = parse_document(source).expect("parse should succeed");
        let definition = resolve_definition(
            "file:///test.ws",
            &document,
            &WorkspaceIndex::default(),
            SourcePosition {
                line: 2,
                character: 1,
            },
        )
        .expect("local variable should resolve");

        let refs = super::find_references(
            &definition,
            &document,
            &[("file:///test.ws", &document)],
            true,
        );
        // Should find x in Outer() only: the declaration and the assignment
        assert_eq!(refs.len(), 2, "x in Other() should not be included");
    }

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
