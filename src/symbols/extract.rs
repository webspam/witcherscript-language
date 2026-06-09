use tree_sitter::Node;

use crate::cst::nav::{first_child_kind, nth_child_kind};
use crate::line_index::LineIndex;
use crate::types::Type;

use super::types::{AccessLevel, Annotation, DocumentSymbols, Symbol, SymbolId, SymbolKind};
use super::util::{base_type, callable_signature, direct_child_text, node_text};

pub fn extract_symbols(root: Node, source: &str, line_index: &LineIndex) -> DocumentSymbols {
    let mut extractor = SymbolExtractor {
        source,
        line_index,
        symbols: DocumentSymbols::default(),
    };

    extractor.visit_children(root, None, Vec::new());
    extractor.symbols.build_indexes();
    extractor.symbols
}

struct SymbolExtractor<'a> {
    source: &'a str,
    line_index: &'a LineIndex,
    symbols: DocumentSymbols,
}

#[derive(Default)]
struct SymbolSpec {
    container: Option<SymbolId>,
    annotations: Vec<Annotation>,
    type_annotation: Option<Type>,
    signature: Option<String>,
    base_class: Option<String>,
    owner_class: Option<String>,
    flavour: Option<String>,
    access: AccessLevel,
    is_optional: bool,
    is_out: bool,
    is_state_machine: bool,
    is_abstract: bool,
}

impl SymbolExtractor<'_> {
    fn visit_children(
        &mut self,
        node: Node,
        container: Option<SymbolId>,
        pending_annotations: Vec<Annotation>,
    ) {
        let mut annotations = pending_annotations;
        let mut cursor = node.walk();

        for child in node
            .children(&mut cursor)
            .filter(tree_sitter::Node::is_named)
        {
            if child.kind() == "annotation" {
                if let Some(annotation) = self.annotation(child) {
                    annotations.push(annotation);
                }
                continue;
            }

            let consumed_annotations = std::mem::take(&mut annotations);
            self.visit(child, container, consumed_annotations);
        }
    }

    fn visit(&mut self, node: Node, container: Option<SymbolId>, annotations: Vec<Annotation>) {
        match node.kind() {
            "class_decl" => self.visit_type_decl(node, container, annotations, SymbolKind::Class),
            "struct_decl" => self.visit_type_decl(node, container, annotations, SymbolKind::Struct),
            "enum_decl" => self.visit_enum_decl(node, container, annotations),
            "state_decl" => self.visit_state_decl(node, container, annotations),
            "func_decl" => {
                self.visit_callable_decl(node, container, annotations, SymbolKind::Function);
            }
            "event_decl" => {
                self.visit_callable_decl(node, container, annotations, SymbolKind::Event);
            }
            "member_var_decl" | "autobind_decl" => {
                self.visit_var_decl(node, container, annotations, SymbolKind::Field);
            }
            "local_var_decl_stmt" => {
                self.visit_var_decl(node, container, annotations, SymbolKind::Variable);
            }
            _ => self.visit_children(node, container, annotations),
        }
    }

    fn visit_type_decl(
        &mut self,
        node: Node,
        container: Option<SymbolId>,
        mut annotations: Vec<Annotation>,
        kind: SymbolKind,
    ) {
        annotations.extend(self.direct_annotations(node));
        let Some(name_node) = first_child_kind(node, "ident") else {
            self.visit_children(node, container, annotations);
            return;
        };
        let base_class = base_type(node, self.source);
        let (is_state_machine, is_abstract) = {
            let mut c = node.walk();
            let mut sm = false;
            let mut ab = false;
            for child in node.children(&mut c) {
                if child.kind() != "specifier" {
                    continue;
                }
                match &self.source[child.start_byte()..child.end_byte()] {
                    "statemachine" => sm = true,
                    "abstract" => ab = true,
                    _ => {}
                }
            }
            (sm, ab)
        };
        let id = self.push_symbol(
            node,
            name_node,
            kind,
            SymbolSpec {
                container,
                annotations,
                base_class,
                is_state_machine,
                is_abstract,
                ..Default::default()
            },
        );

        self.visit_children(node, Some(id), Vec::new());
    }

    fn visit_enum_decl(
        &mut self,
        node: Node,
        container: Option<SymbolId>,
        mut annotations: Vec<Annotation>,
    ) {
        annotations.extend(self.direct_annotations(node));
        let Some(name_node) = first_child_kind(node, "ident") else {
            self.visit_children(node, container, annotations);
            return;
        };
        let enum_id = self.push_symbol(
            node,
            name_node,
            SymbolKind::Enum,
            SymbolSpec {
                container,
                annotations,
                ..Default::default()
            },
        );

        self.visit_enum_members(node, enum_id);
    }

    fn visit_enum_members(&mut self, node: Node, enum_id: SymbolId) {
        let mut cursor = node.walk();
        for child in node
            .children(&mut cursor)
            .filter(tree_sitter::Node::is_named)
        {
            if child.kind() == "enum_decl_variant" {
                if let Some(name_node) = first_child_kind(child, "ident") {
                    self.push_symbol(
                        child,
                        name_node,
                        SymbolKind::EnumMember,
                        SymbolSpec {
                            container: Some(enum_id),
                            ..Default::default()
                        },
                    );
                }
            } else {
                self.visit_enum_members(child, enum_id);
            }
        }
    }

    fn visit_state_decl(
        &mut self,
        node: Node,
        container: Option<SymbolId>,
        mut annotations: Vec<Annotation>,
    ) {
        annotations.extend(self.direct_annotations(node));
        let Some(name_node) = first_child_kind(node, "ident") else {
            self.visit_children(node, container, annotations);
            return;
        };
        let owner_class = nth_child_kind(node, "ident", 1).map(|n| node_text(n, self.source));
        let base_class = node
            .child_by_field_name("base")
            .map(|n| node_text(n, self.source));
        let id = self.push_symbol(
            node,
            name_node,
            SymbolKind::State,
            SymbolSpec {
                container,
                annotations,
                base_class,
                owner_class,
                ..Default::default()
            },
        );

        self.visit_children(node, Some(id), Vec::new());
    }

    fn visit_callable_decl(
        &mut self,
        node: Node,
        container: Option<SymbolId>,
        mut annotations: Vec<Annotation>,
        default_kind: SymbolKind,
    ) {
        annotations.extend(self.direct_annotations(node));
        let Some(name_node) = first_child_kind(node, "ident") else {
            self.visit_children(node, container, annotations);
            return;
        };
        let kind = if default_kind == SymbolKind::Function && container.is_some() {
            SymbolKind::Method
        } else {
            default_kind
        };
        let signature = callable_signature(node, self.source);
        let type_annotation =
            direct_child_text(node, "type_annot", self.source).map(|t| Type::from_annotation(&t));
        let flavour = first_child_kind(node, "func_flavour").map(|n| node_text(n, self.source));
        let access = self.node_access_level(node);
        let id = self.push_symbol(
            node,
            name_node,
            kind,
            SymbolSpec {
                container,
                annotations,
                type_annotation,
                signature,
                flavour,
                access,
                ..Default::default()
            },
        );

        self.visit_params(node, id);
        self.visit_body_locals(node, id);
    }

    fn visit_params(&mut self, node: Node, function_id: SymbolId) {
        if let Some(params) = first_child_kind(node, "func_params") {
            let mut cursor = params.walk();
            for group in params
                .children(&mut cursor)
                .filter(|child| child.kind() == "func_param_group")
            {
                self.visit_var_decl(group, Some(function_id), Vec::new(), SymbolKind::Parameter);
            }
        }
    }

    fn visit_body_locals(&mut self, node: Node, function_id: SymbolId) {
        if let Some(block) = first_child_kind(node, "func_block") {
            self.visit_children(block, Some(function_id), Vec::new());
        }
    }

    fn visit_var_decl(
        &mut self,
        node: Node,
        container: Option<SymbolId>,
        mut annotations: Vec<Annotation>,
        kind: SymbolKind,
    ) {
        annotations.extend(self.direct_annotations(node));
        let type_annotation =
            direct_child_text(node, "type_annot", self.source).map(|t| Type::from_annotation(&t));
        let field_signature = if kind == SymbolKind::Field {
            Some(node_text(node, self.source))
        } else {
            None
        };
        let access = self.node_access_level(node);
        let is_optional = {
            let mut c = node.walk();

            node.children(&mut c).any(|child| {
                child.kind() == "specifier"
                    && &self.source[child.start_byte()..child.end_byte()] == "optional"
            })
        };
        let is_out = {
            let mut c = node.walk();

            node.children(&mut c).any(|child| {
                child.kind() == "specifier"
                    && &self.source[child.start_byte()..child.end_byte()] == "out"
            })
        };
        let mut cursor = node.walk();
        let names_field = if node.kind() == "autobind_decl" {
            "name"
        } else {
            "names"
        };

        for child in node.children_by_field_name(names_field, &mut cursor) {
            if child.kind() == "ident" {
                self.push_symbol(
                    node,
                    child,
                    kind,
                    SymbolSpec {
                        container,
                        annotations: annotations.clone(),
                        type_annotation: type_annotation.clone(),
                        signature: field_signature.clone(),
                        access,
                        is_optional,
                        is_out,
                        ..Default::default()
                    },
                );
            }
        }
    }

    fn annotation(&self, node: Node) -> Option<Annotation> {
        let name = first_child_kind(node, "annotation_ident").map(|name| {
            node_text(name, self.source)
                .trim_start_matches('@')
                .to_string()
        })?;
        let argument = first_child_kind(node, "ident").map(|arg| node_text(arg, self.source));

        Some(Annotation { name, argument })
    }

    fn direct_annotations(&self, node: Node) -> Vec<Annotation> {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .filter(|child| child.kind() == "annotation")
            .filter_map(|child| self.annotation(child))
            .collect()
    }

    fn push_symbol(
        &mut self,
        node: Node,
        name_node: Node,
        kind: SymbolKind,
        spec: SymbolSpec,
    ) -> SymbolId {
        let container_name = spec
            .container
            .and_then(|id| self.symbols.by_id(id))
            .map(|s| s.name.clone());
        self.symbols.push(Symbol {
            id: SymbolId(usize::MAX),
            name: node_text(name_node, self.source),
            kind,
            range: self.line_index.byte_range_to_range(
                self.source,
                node.start_byte(),
                node.end_byte(),
            ),
            selection_range: self.line_index.byte_range_to_range(
                self.source,
                name_node.start_byte(),
                name_node.end_byte(),
            ),
            byte_range: node.start_byte()..node.end_byte(),
            selection_byte_range: name_node.start_byte()..name_node.end_byte(),
            container: spec.container,
            container_name,
            type_annotation: spec.type_annotation,
            signature: spec.signature,
            base_class: spec.base_class,
            owner_class: spec.owner_class,
            flavour: spec.flavour,
            annotations: spec.annotations,
            access: spec.access,
            is_optional: spec.is_optional,
            is_out: spec.is_out,
            is_state_machine: spec.is_state_machine,
            is_abstract: spec.is_abstract,
        })
    }

    fn node_access_level(&self, node: Node) -> AccessLevel {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "specifier" {
                match &self.source[child.start_byte()..child.end_byte()] {
                    "private" => return AccessLevel::Private,
                    "protected" => return AccessLevel::Protected,
                    _ => {}
                }
            }
        }
        AccessLevel::Public
    }
}
