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
        self.find_member_in_chain(container_name, name, 0)
    }

    fn find_member_in_chain(
        &self,
        container_name: &str,
        name: &str,
        depth: usize,
    ) -> Option<Definition> {
        if depth > 32 {
            return None;
        }
        let direct = self.documents.iter().find_map(|(uri, symbols)| {
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
        });
        if direct.is_some() {
            return direct;
        }
        let superclass = self.superclass_of(container_name)?;
        self.find_member_in_chain(&superclass, name, depth + 1)
    }

    fn superclass_of(&self, name: &str) -> Option<String> {
        self.documents.iter().find_map(|(_, symbols)| {
            symbols
                .iter()
                .find(|s| s.name == name && is_type_like(s.kind))
                .and_then(|s| {
                    s.detail
                        .as_deref()
                        .and_then(|d| d.strip_prefix("extends "))
                        .map(|base| base.to_string())
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

    if let Some(def) = resolve_super_keyword(uri, document, workspace, byte_offset) {
        return Some(def);
    }

    let ident = identifier_at(document.tree.root_node(), byte_offset)?;
    let name = ident.utf8_text(document.source.as_bytes()).ok()?;

    if let Some(member_definition) = resolve_member_access(uri, document, workspace, ident, name) {
        return Some(member_definition);
    }

    resolve_local_or_parameter(uri, document, byte_offset, name)
        .or_else(|| resolve_current_type_member(uri, document, workspace, byte_offset, name))
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
    workspace: &WorkspaceIndex,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    let current_type = current_type_name(document, byte_offset)?;
    resolve_document_member(uri, document, &current_type, name)
        .or_else(|| workspace.find_member(&current_type, name))
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
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
    include_declaration: bool,
) -> Vec<(String, SourceRange)> {
    let name = &definition.symbol.name;

    // Variables and parameters cannot be referenced outside their file.
    let locals_only = matches!(
        definition.symbol.kind,
        SymbolKind::Variable | SymbolKind::Parameter
    );

    // For locals/parameters, restrict the text scan to the enclosing function's
    // byte range so we skip irrelevant regions quickly before semantic verification.
    let scan_scope: Option<std::ops::Range<usize>> = if locals_only {
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
        if locals_only && *uri != definition.uri.as_str() {
            continue;
        }

        let mut byte_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        collect_ident_occurrences(
            document.tree.root_node(),
            document.source.as_bytes(),
            name,
            scan_scope.as_ref(),
            &mut byte_ranges,
        );

        for byte_range in byte_ranges {
            // Semantic verification: resolve the candidate and confirm it points
            // at the same definition (same file + same selection range).
            let position = document
                .line_index
                .byte_to_position(&document.source, byte_range.start);
            let resolved = resolve_definition(uri, document, workspace, position)
                .or_else(|| resolve_definition(uri, document, base, position));
            match resolved {
                Some(ref r)
                    if r.uri == definition.uri
                        && r.symbol.selection_byte_range
                            == definition.symbol.selection_byte_range => {}
                _ => continue,
            }

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

fn resolve_super_keyword(
    uri: &str,
    document: &ParsedDocument,
    workspace: &WorkspaceIndex,
    byte_offset: usize,
) -> Option<Definition> {
    let root = document.tree.root_node();
    let node = [byte_offset, byte_offset.saturating_sub(1)]
        .into_iter()
        .find_map(|offset| root.descendant_for_byte_range(offset, offset))?;

    let is_super = matches!(node.kind(), "super" | "parent")
        || matches!(
            node.parent().map(|p| p.kind()),
            Some("super_expr" | "parent_expr")
        );
    if !is_super {
        return None;
    }

    let current_type = current_type_symbol(document, byte_offset)?;
    let base_name = current_type
        .detail
        .as_deref()
        .and_then(|d| d.strip_prefix("extends "))?;

    resolve_document_top_level(uri, document, base_name)
        .or_else(|| workspace.find_top_level(base_name))
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

        let mut index = WorkspaceIndex::default();
        index.update_document("file:///test.ws", &document.symbols);

        let refs = super::find_references(
            &definition,
            &document,
            &[("file:///test.ws", &document)],
            &index,
            &WorkspaceIndex::default(),
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

        let mut index = WorkspaceIndex::default();
        index.update_document("file:///test.ws", &document.symbols);

        let with_decl = super::find_references(
            &definition,
            &document,
            &[("file:///test.ws", &document)],
            &index,
            &WorkspaceIndex::default(),
            true,
        );
        let without_decl = super::find_references(
            &definition,
            &document,
            &[("file:///test.ws", &document)],
            &index,
            &WorkspaceIndex::default(),
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

        let mut index = WorkspaceIndex::default();
        index.update_document("file:///test.ws", &document.symbols);

        let refs = super::find_references(
            &definition,
            &document,
            &[("file:///test.ws", &document)],
            &index,
            &WorkspaceIndex::default(),
            true,
        );
        // Should find x in Outer() only: the declaration and the assignment
        assert_eq!(refs.len(), 2, "x in Other() should not be included");
    }

    #[test]
    fn resolves_super_keyword_to_parent_class() {
        let source_a = "class A extends B {\n function Test() {\n  super.Method();\n }\n}\n";
        let source_b = "class B {\n function Method() {}\n}\n";
        let doc_a = parse_document(source_a).expect("parse should succeed");
        let doc_b = parse_document(source_b).expect("parse should succeed");

        let mut index = WorkspaceIndex::default();
        index.update_document("file:///a.ws", &doc_a.symbols);
        index.update_document("file:///b.ws", &doc_b.symbols);

        // cursor on 'super' (line 2, col 3)
        let definition = resolve_definition(
            "file:///a.ws",
            &doc_a,
            &index,
            SourcePosition {
                line: 2,
                character: 3,
            },
        )
        .expect("super keyword should navigate to parent class");

        assert_eq!(definition.symbol.name, "B");
        assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Class);
    }

    #[test]
    fn resolves_inherited_method_via_workspace() {
        let source_a = "class A extends B {\n function Test() {\n  Inherited();\n }\n}\n";
        let source_b = "class B {\n function Inherited() {}\n}\n";
        let doc_a = parse_document(source_a).expect("parse should succeed");
        let doc_b = parse_document(source_b).expect("parse should succeed");

        let mut index = WorkspaceIndex::default();
        index.update_document("file:///a.ws", &doc_a.symbols);
        index.update_document("file:///b.ws", &doc_b.symbols);

        let definition = resolve_definition(
            "file:///a.ws",
            &doc_a,
            &index,
            SourcePosition {
                line: 2,
                character: 3,
            },
        )
        .expect("inherited method should resolve");

        assert_eq!(definition.symbol.name, "Inherited");
        assert_eq!(definition.symbol.kind, crate::symbols::SymbolKind::Method);
    }

    #[test]
    fn class_without_explicit_extends_defaults_to_cobject() {
        let doc = parse_document("class A {}").expect("parse should succeed");
        let mut index = WorkspaceIndex::default();
        index.update_document("file:///a.ws", &doc.symbols);
        // CObject is not in the index; find_member must terminate without looping.
        assert!(index.find_member("A", "someMethod").is_none());
    }

    #[test]
    fn resolves_inherited_method_unqualified_inside_subclass() {
        let source_a = "class A extends B {\n function Test() {\n  Inherited();\n }\n}\n";
        let source_b = "class B {\n function Inherited() {}\n}\n";
        let doc_a = parse_document(source_a).expect("parse should succeed");
        let doc_b = parse_document(source_b).expect("parse should succeed");

        let mut index = WorkspaceIndex::default();
        index.update_document("file:///a.ws", &doc_a.symbols);
        index.update_document("file:///b.ws", &doc_b.symbols);

        let definition = resolve_definition(
            "file:///a.ws",
            &doc_a,
            &index,
            SourcePosition {
                line: 2,
                character: 3,
            },
        )
        .expect("unqualified inherited method should resolve inside subclass body");

        assert_eq!(definition.symbol.name, "Inherited");
    }

    #[test]
    fn resolves_this_dot_inherited_method() {
        let source_a = "class A extends B {\n function Test() {\n  this.Inherited();\n }\n}\n";
        let source_b = "class B {\n function Inherited() {}\n}\n";
        let doc_a = parse_document(source_a).expect("parse should succeed");
        let doc_b = parse_document(source_b).expect("parse should succeed");

        let mut index = WorkspaceIndex::default();
        index.update_document("file:///a.ws", &doc_a.symbols);
        index.update_document("file:///b.ws", &doc_b.symbols);

        let definition = resolve_definition(
            "file:///a.ws",
            &doc_a,
            &index,
            SourcePosition {
                line: 2,
                character: 8,
            },
        )
        .expect("this.Inherited() should resolve to superclass method");

        assert_eq!(definition.symbol.name, "Inherited");
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
