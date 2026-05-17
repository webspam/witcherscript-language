use tree_sitter::Node;

use crate::line_index::{LineIndex, SourceRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum AccessLevel {
    Private,
    Protected,
    #[default]
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    Struct,
    Enum,
    EnumVariant,
    Function,
    Method,
    Field,
    Variable,
    Parameter,
    State,
    Event,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub name: String,
    pub argument: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub range: SourceRange,
    pub selection_range: SourceRange,
    pub byte_range: std::ops::Range<usize>,
    pub selection_byte_range: std::ops::Range<usize>,
    pub container: Option<SymbolId>,
    pub container_name: Option<String>,
    pub type_annotation: Option<String>,
    pub signature: Option<String>,
    pub detail: Option<String>,
    pub base_class: Option<String>,
    pub owner_class: Option<String>,
    pub flavour: Option<String>,
    pub annotations: Vec<Annotation>,
    pub access: AccessLevel,
    pub is_optional: bool,
    pub is_out: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DocumentSymbols {
    symbols: Vec<Symbol>,
}

impl DocumentSymbols {
    pub fn all(&self) -> &[Symbol] {
        &self.symbols
    }

    pub fn by_id(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id.0)
    }

    pub fn children_of(&self, id: Option<SymbolId>) -> impl Iterator<Item = &Symbol> {
        self.symbols
            .iter()
            .filter(move |symbol| symbol.container == id)
    }

    pub fn enclosing_symbol_at(&self, byte_offset: usize, kinds: &[SymbolKind]) -> Option<&Symbol> {
        self.symbols
            .iter()
            .filter(|symbol| {
                kinds.contains(&symbol.kind)
                    && symbol.byte_range.start <= byte_offset
                    && byte_offset <= symbol.byte_range.end
            })
            .min_by_key(|symbol| symbol.byte_range.end - symbol.byte_range.start)
    }

    pub fn mark_optional(&mut self, id: SymbolId) {
        if let Some(sym) = self.symbols.get_mut(id.0) {
            sym.is_optional = true;
        }
    }

    pub fn mark_out(&mut self, id: SymbolId) {
        if let Some(sym) = self.symbols.get_mut(id.0) {
            sym.is_out = true;
        }
    }

    fn push(&mut self, mut symbol: Symbol) -> SymbolId {
        let id = SymbolId(self.symbols.len());
        symbol.id = id;
        self.symbols.push(symbol);
        id
    }
}

pub fn extract_symbols(root: Node, source: &str, line_index: &LineIndex) -> DocumentSymbols {
    let mut extractor = SymbolExtractor {
        source,
        line_index,
        symbols: DocumentSymbols::default(),
    };

    extractor.visit_children(root, None, Vec::new());
    extractor.symbols
}

struct SymbolExtractor<'a> {
    source: &'a str,
    line_index: &'a LineIndex,
    symbols: DocumentSymbols,
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

        for child in node.children(&mut cursor).filter(|child| child.is_named()) {
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
                self.visit_callable_decl(node, container, annotations, SymbolKind::Function)
            }
            "event_decl" => {
                self.visit_callable_decl(node, container, annotations, SymbolKind::Event)
            }
            "member_var_decl" => {
                self.visit_var_decl(node, container, annotations, SymbolKind::Field)
            }
            "local_var_decl_stmt" => {
                self.visit_var_decl(node, container, annotations, SymbolKind::Variable)
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
        let detail = base_class.as_deref().map(|b| format!("extends {b}"));
        let id = self.push_symbol(
            node,
            name_node,
            container,
            annotations,
            kind,
            None,
            None,
            detail,
            base_class,
            None,
            None,
            AccessLevel::Public,
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
            container,
            annotations,
            SymbolKind::Enum,
            None,
            None,
            None,
            None,
            None,
            None,
            AccessLevel::Public,
        );

        self.visit_enum_variants(node, enum_id);
    }

    fn visit_enum_variants(&mut self, node: Node, enum_id: SymbolId) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor).filter(|child| child.is_named()) {
            if child.kind() == "enum_decl_variant" {
                if let Some(name_node) = first_child_kind(child, "ident") {
                    self.push_symbol(
                        child,
                        name_node,
                        Some(enum_id),
                        Vec::new(),
                        SymbolKind::EnumVariant,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        AccessLevel::Public,
                    );
                }
            } else {
                self.visit_enum_variants(child, enum_id);
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
        let detail = match (&owner_class, &base_class) {
            (Some(o), Some(b)) => Some(format!("in {o} extends {b}")),
            (Some(o), None) => Some(format!("in {o}")),
            (None, Some(b)) => Some(format!("extends {b}")),
            (None, None) => None,
        };
        let id = self.push_symbol(
            node,
            name_node,
            container,
            annotations,
            SymbolKind::State,
            None,
            None,
            detail,
            base_class,
            owner_class,
            None,
            AccessLevel::Public,
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
        let type_annotation = direct_child_text(node, "type_annot", self.source);
        let flavour = first_child_kind(node, "func_flavour").map(|n| node_text(n, self.source));
        let access = self.node_access_level(node);
        let id = self.push_symbol(
            node,
            name_node,
            container,
            annotations,
            kind,
            type_annotation,
            signature,
            None,
            None,
            None,
            flavour,
            access,
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
        let type_annotation = direct_child_text(node, "type_annot", self.source);
        let field_signature = if kind == SymbolKind::Field {
            Some(node_text(node, self.source))
        } else {
            None
        };
        let access = self.node_access_level(node);
        let is_optional = {
            let mut c = node.walk();
            let result = node.children(&mut c).any(|child| {
                child.kind() == "specifier"
                    && &self.source[child.start_byte()..child.end_byte()] == "optional"
            });
            result
        };
        let is_out = {
            let mut c = node.walk();
            let result = node.children(&mut c).any(|child| {
                child.kind() == "specifier"
                    && &self.source[child.start_byte()..child.end_byte()] == "out"
            });
            result
        };
        let mut cursor = node.walk();

        for child in node.children_by_field_name("names", &mut cursor) {
            if child.kind() == "ident" {
                let id = self.push_symbol(
                    node,
                    child,
                    container,
                    annotations.clone(),
                    kind,
                    type_annotation.clone(),
                    field_signature.clone(),
                    None,
                    None,
                    None,
                    None,
                    access,
                );
                if is_optional {
                    self.symbols.mark_optional(id);
                }
                if is_out {
                    self.symbols.mark_out(id);
                }
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

    #[allow(clippy::too_many_arguments)]
    fn push_symbol(
        &mut self,
        node: Node,
        name_node: Node,
        container: Option<SymbolId>,
        annotations: Vec<Annotation>,
        kind: SymbolKind,
        type_annotation: Option<String>,
        signature: Option<String>,
        detail: Option<String>,
        base_class: Option<String>,
        owner_class: Option<String>,
        flavour: Option<String>,
        access: AccessLevel,
    ) -> SymbolId {
        let container_name = container
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
            container,
            container_name,
            type_annotation,
            signature,
            detail,
            base_class,
            owner_class,
            flavour,
            annotations,
            access,
            is_optional: false,
            is_out: false,
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

pub fn node_text(node: Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

pub fn first_child_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    nth_child_kind(node, kind, 0)
}

pub fn nth_child_kind<'tree>(node: Node<'tree>, kind: &str, index: usize) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let child = node
        .children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .nth(index);
    child
}

fn direct_child_text(node: Node, kind: &str, source: &str) -> Option<String> {
    first_child_kind(node, kind).map(|child| node_text(child, source))
}

fn callable_signature(node: Node, source: &str) -> Option<String> {
    crate::formatter::render_callable_signature(node, source)
}

fn base_type(node: Node, source: &str) -> Option<String> {
    nth_child_kind(node, "ident", 1).map(|base| node_text(base, source))
}

#[cfg(test)]
mod tests {
    use tree_sitter::Parser;

    use super::{extract_symbols, SymbolKind};
    use crate::line_index::LineIndex;

    #[test]
    fn extracts_functions_params_and_locals() {
        let source =
            "function Basic(owner : CObject) : bool {\n var count : int;\n return true;\n}\n";
        let tree = parse(source);
        let symbols = extract_symbols(tree.root_node(), source, &LineIndex::new(source));

        assert!(symbols
            .all()
            .iter()
            .any(|symbol| symbol.name == "Basic" && symbol.kind == SymbolKind::Function));
        assert!(symbols
            .all()
            .iter()
            .any(|symbol| symbol.name == "owner" && symbol.kind == SymbolKind::Parameter));
        assert!(symbols
            .all()
            .iter()
            .any(|symbol| symbol.name == "count" && symbol.kind == SymbolKind::Variable));
    }

    #[test]
    fn var_decl_initializer_ident_is_not_recorded_as_local() {
        let cases: &[(&str, &str, &[&str])] = &[
            (
                "single name with ident initializer",
                "function F() { var x : int = name; }\n",
                &["x"],
            ),
            (
                "multi-name decl with ident initializer",
                "function F() { var x, y : int = name; }\n",
                &["x", "y"],
            ),
            (
                "initializer references a prior local",
                "function F() {\n var source : int;\n var x : int = source;\n}\n",
                &["source", "x"],
            ),
        ];

        for (msg, source, expected) in cases {
            let tree = parse(source);
            let symbols = extract_symbols(tree.root_node(), source, &LineIndex::new(source));
            let vars: Vec<&str> = symbols
                .all()
                .iter()
                .filter(|s| s.kind == SymbolKind::Variable)
                .map(|s| s.name.as_str())
                .collect();
            assert_eq!(&vars[..], *expected, "{msg}");
        }
    }

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_witcherscript::language())
            .expect("failed to load WitcherScript grammar");
        parser.parse(source, None).expect("failed to parse source")
    }
}
