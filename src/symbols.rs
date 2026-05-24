use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::nav::{first_child_kind, nth_child_kind};
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
    pub base_class: Option<String>,
    pub owner_class: Option<String>,
    pub flavour: Option<String>,
    pub annotations: Vec<Annotation>,
    pub access: AccessLevel,
    pub is_optional: bool,
    pub is_out: bool,
    pub is_state_machine: bool,
    pub is_abstract: bool,
}

impl Symbol {
    pub fn display_detail(&self) -> Option<String> {
        match (self.base_class.as_deref(), self.owner_class.as_deref()) {
            (Some(b), Some(o)) => Some(format!("in {o} extends {b}")),
            (Some(b), None) => Some(format!("extends {b}")),
            (None, Some(o)) => Some(format!("in {o}")),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DocumentSymbols {
    symbols: Vec<Symbol>,
    by_start_byte: Vec<SymbolId>,
    top_level_by_name: HashMap<String, Vec<SymbolId>>,
    type_by_name: HashMap<String, SymbolId>,
    members_by_container: HashMap<SymbolId, HashMap<String, Vec<SymbolId>>>,
    locals_in_function: HashMap<SymbolId, HashMap<String, Vec<SymbolId>>>,
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
        let upper = self
            .by_start_byte
            .partition_point(|id| self.symbols[id.0].byte_range.start <= byte_offset);
        if upper == 0 {
            return None;
        }
        let mut cursor: Option<SymbolId> = Some(self.by_start_byte[upper - 1]);
        while let Some(id) = cursor {
            let sym = &self.symbols[id.0];
            if byte_offset <= sym.byte_range.end && kinds.contains(&sym.kind) {
                return Some(sym);
            }
            cursor = sym.container;
        }
        None
    }

    pub fn top_level_by_name(&self, name: &str) -> Option<&Symbol> {
        let ids = self.top_level_by_name.get(name)?;
        ids.first().map(|id| &self.symbols[id.0])
    }

    pub fn type_by_name(&self, name: &str) -> Option<&Symbol> {
        self.type_by_name.get(name).map(|id| &self.symbols[id.0])
    }

    pub fn member_of(&self, container: SymbolId, name: &str) -> impl Iterator<Item = &Symbol> {
        self.members_by_container
            .get(&container)
            .and_then(|by_name| by_name.get(name))
            .map(|ids| ids.iter())
            .into_iter()
            .flatten()
            .map(|id| &self.symbols[id.0])
    }

    pub fn local_at_byte(
        &self,
        function: SymbolId,
        name: &str,
        before_byte: usize,
    ) -> Option<&Symbol> {
        let by_name = self.locals_in_function.get(&function)?;
        let ids = by_name.get(name)?;
        for id in ids.iter().rev() {
            let sym = &self.symbols[id.0];
            if sym.selection_byte_range.start <= before_byte {
                return Some(sym);
            }
        }
        None
    }

    fn push(&mut self, mut symbol: Symbol) -> SymbolId {
        let id = SymbolId(self.symbols.len());
        symbol.id = id;
        self.symbols.push(symbol);
        id
    }

    fn build_indexes(&mut self) {
        let mut by_start: Vec<SymbolId> = (0..self.symbols.len()).map(SymbolId).collect();
        by_start.sort_by_key(|id| self.symbols[id.0].byte_range.start);
        self.by_start_byte = by_start;

        for sym in &self.symbols {
            match sym.container {
                None => {
                    self.top_level_by_name
                        .entry(sym.name.clone())
                        .or_default()
                        .push(sym.id);
                }
                Some(container) => {
                    self.members_by_container
                        .entry(container)
                        .or_default()
                        .entry(sym.name.clone())
                        .or_default()
                        .push(sym.id);
                }
            }
            if matches!(
                sym.kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
            ) {
                self.type_by_name.entry(sym.name.clone()).or_insert(sym.id);
            }
        }

        for sym in &self.symbols {
            if !matches!(sym.kind, SymbolKind::Variable | SymbolKind::Parameter) {
                continue;
            }
            let Some(function) = enclosing_callable_id(&self.symbols, sym) else {
                continue;
            };
            self.locals_in_function
                .entry(function)
                .or_default()
                .entry(sym.name.clone())
                .or_default()
                .push(sym.id);
        }
        for by_name in self.locals_in_function.values_mut() {
            for ids in by_name.values_mut() {
                ids.sort_by_key(|id| self.symbols[id.0].selection_byte_range.start);
            }
        }
    }
}

pub(crate) fn enclosing_callable_id(symbols: &[Symbol], sym: &Symbol) -> Option<SymbolId> {
    let mut current = sym.container?;
    loop {
        let owner = symbols.get(current.0)?;
        if matches!(
            owner.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Event
        ) {
            return Some(current);
        }
        current = owner.container?;
    }
}

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
    type_annotation: Option<String>,
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
            "member_var_decl" | "autobind_decl" => {
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
                        SymbolKind::EnumVariant,
                        SymbolSpec {
                            container: Some(enum_id),
                            ..Default::default()
                        },
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
        let type_annotation = direct_child_text(node, "type_annot", self.source);
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

pub fn node_text(node: Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
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
mod tests;
