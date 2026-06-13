use tree_sitter::Node;

use crate::cst::nav::{first_child_kind, nth_child_kind};
use crate::cst::walk::{CstVisitor, Visit, walk};
use crate::cst::{fields, kinds};
use crate::line_index::LineIndex;
use crate::types::Type;

use super::types::{
    AccessLevel, Annotation, DocumentSymbols, FuncFlavour, Specifier, Specifiers, Symbol, SymbolId,
    SymbolKind,
};
use super::util::{base_type, direct_child_text, node_text};

pub fn extract_symbols(root: Node, source: &str, line_index: &LineIndex) -> DocumentSymbols {
    let mut extractor = SymbolExtractor::new(source, line_index);
    walk(root, &mut extractor);
    extractor.finish()
}

pub(crate) struct SymbolExtractor<'a> {
    source: &'a str,
    line_index: &'a LineIndex,
    symbols: DocumentSymbols,
    depth: usize,
    frames: Vec<Frame>,
}

// One frame per named node the walk descends into; popped on that node's leave.
struct Frame {
    depth: usize,
    mode: Mode,
}

enum Mode {
    // Stray sibling annotations pend here and die with the frame.
    Body {
        container: Option<SymbolId>,
        pending: Vec<Annotation>,
    },
    // Inside an enum, only members are extracted; nested decls are not symbols.
    EnumMembers(SymbolId),
    // Params were pushed at the decl's enter; only the first func_block opens a body.
    Callable {
        id: SymbolId,
        body_seen: bool,
    },
}

#[derive(Default)]
struct SymbolSpec {
    container: Option<SymbolId>,
    annotations: Vec<Annotation>,
    type_annotation: Option<Type>,
    base_class: Option<String>,
    owner_class: Option<String>,
    flavour: Option<FuncFlavour>,
    access: AccessLevel,
    specifiers: Specifiers,
}

impl<'tree> CstVisitor<'tree> for SymbolExtractor<'_> {
    fn enter(&mut self, node: Node<'tree>) -> Visit {
        let visit = self.enter_node(node);
        self.depth += 1;
        visit
    }

    fn leave(&mut self, _node: Node<'tree>) {
        self.depth -= 1;
        if self
            .frames
            .last()
            .is_some_and(|frame| frame.depth == self.depth)
        {
            self.frames.pop();
        }
    }
}

impl<'a> SymbolExtractor<'a> {
    pub(crate) fn new(source: &'a str, line_index: &'a LineIndex) -> Self {
        Self {
            source,
            line_index,
            symbols: DocumentSymbols::default(),
            depth: 0,
            frames: Vec::new(),
        }
    }

    pub(crate) fn finish(mut self) -> DocumentSymbols {
        self.symbols.build_indexes();
        self.symbols
    }

    fn enter_node(&mut self, node: Node) -> Visit {
        if self.frames.is_empty() {
            // The walk root is a pure container, never a declaration itself.
            self.push_frame(Mode::Body {
                container: None,
                pending: Vec::new(),
            });
            return Visit::Children;
        }
        if !node.is_named() {
            return Visit::SkipChildren;
        }
        let innermost = self.frames.len() - 1;
        match self.frames[innermost].mode {
            Mode::Body { container, .. } => self.enter_in_body(node, container),
            Mode::EnumMembers(enum_id) => self.enter_in_enum(node, enum_id),
            Mode::Callable { id, body_seen } => self.enter_in_callable(node, id, body_seen),
        }
    }

    fn push_frame(&mut self, mode: Mode) {
        self.frames.push(Frame {
            depth: self.depth,
            mode,
        });
    }

    fn take_pending(&mut self) -> Vec<Annotation> {
        match self.frames.last_mut().map(|frame| &mut frame.mode) {
            Some(Mode::Body { pending, .. }) => std::mem::take(pending),
            _ => unreachable!("pending annotations only exist in Body frames"),
        }
    }

    fn pend_annotation(&mut self, annotation: Annotation) {
        match self.frames.last_mut().map(|frame| &mut frame.mode) {
            Some(Mode::Body { pending, .. }) => pending.push(annotation),
            _ => unreachable!("pending annotations only exist in Body frames"),
        }
    }

    fn enter_in_body(&mut self, node: Node, container: Option<SymbolId>) -> Visit {
        match node.kind() {
            kinds::ANNOTATION => {
                if let Some(annotation) = self.annotation(node) {
                    self.pend_annotation(annotation);
                }
                Visit::SkipChildren
            }
            kinds::CLASS_DECL => self.enter_type_decl(node, container, SymbolKind::Class),
            kinds::STRUCT_DECL => self.enter_type_decl(node, container, SymbolKind::Struct),
            kinds::ENUM_DECL => self.enter_enum_decl(node, container),
            kinds::STATE_DECL => self.enter_state_decl(node, container),
            kinds::FUNC_DECL => self.enter_callable_decl(node, container, SymbolKind::Function),
            kinds::EVENT_DECL => self.enter_callable_decl(node, container, SymbolKind::Event),
            kinds::MEMBER_VAR_DECL | kinds::AUTOBIND_DECL => {
                let annotations = self.take_pending();
                self.visit_var_decl(node, container, annotations, SymbolKind::Field);
                Visit::SkipChildren
            }
            kinds::LOCAL_VAR_DECL_STMT => {
                let annotations = self.take_pending();
                self.visit_var_decl(node, container, annotations, SymbolKind::Variable);
                Visit::SkipChildren
            }
            _ => {
                // Pending annotations forward into the next named sibling's level and die there.
                let pending = self.take_pending();
                self.push_frame(Mode::Body { container, pending });
                Visit::Children
            }
        }
    }

    fn enter_in_enum(&mut self, node: Node, enum_id: SymbolId) -> Visit {
        if node.kind() == kinds::ENUM_MEMBER_DECL {
            if let Some(name_node) = first_child_kind(node, kinds::IDENT) {
                self.push_symbol(
                    node,
                    name_node,
                    SymbolKind::EnumMember,
                    SymbolSpec {
                        container: Some(enum_id),
                        ..Default::default()
                    },
                );
            }
            return Visit::SkipChildren;
        }
        self.push_frame(Mode::EnumMembers(enum_id));
        Visit::Children
    }

    fn enter_in_callable(&mut self, node: Node, id: SymbolId, body_seen: bool) -> Visit {
        if node.kind() == kinds::FUNC_BLOCK && !body_seen {
            match self.frames.last_mut().map(|frame| &mut frame.mode) {
                Some(Mode::Callable { body_seen, .. }) => *body_seen = true,
                _ => unreachable!("enter_in_callable runs under a Callable frame"),
            }
            self.push_frame(Mode::Body {
                container: Some(id),
                pending: Vec::new(),
            });
            return Visit::Children;
        }
        Visit::SkipChildren
    }

    fn enter_type_decl(
        &mut self,
        node: Node,
        container: Option<SymbolId>,
        kind: SymbolKind,
    ) -> Visit {
        let mut annotations = self.take_pending();
        annotations.extend(self.direct_annotations(node));
        let Some(name_node) = first_child_kind(node, kinds::IDENT) else {
            self.push_frame(Mode::Body {
                container,
                pending: annotations,
            });
            return Visit::Children;
        };
        let base_class = base_type(node, self.source);
        // Type declarations keep the default public access; only their flag specifiers matter.
        let (_, specifiers) = self.specifiers_of(node);
        let id = self.push_symbol(
            node,
            name_node,
            kind,
            SymbolSpec {
                container,
                annotations,
                base_class,
                specifiers,
                ..Default::default()
            },
        );

        self.push_frame(Mode::Body {
            container: Some(id),
            pending: Vec::new(),
        });
        Visit::Children
    }

    fn enter_enum_decl(&mut self, node: Node, container: Option<SymbolId>) -> Visit {
        let mut annotations = self.take_pending();
        annotations.extend(self.direct_annotations(node));
        let Some(name_node) = first_child_kind(node, kinds::IDENT) else {
            self.push_frame(Mode::Body {
                container,
                pending: annotations,
            });
            return Visit::Children;
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

        self.push_frame(Mode::EnumMembers(enum_id));
        Visit::Children
    }

    fn enter_state_decl(&mut self, node: Node, container: Option<SymbolId>) -> Visit {
        let mut annotations = self.take_pending();
        annotations.extend(self.direct_annotations(node));
        let Some(name_node) = first_child_kind(node, kinds::IDENT) else {
            self.push_frame(Mode::Body {
                container,
                pending: annotations,
            });
            return Visit::Children;
        };
        let owner_class = nth_child_kind(node, kinds::IDENT, 1).map(|n| node_text(n, self.source));
        let base_class = node
            .child_by_field_name(fields::BASE)
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

        self.push_frame(Mode::Body {
            container: Some(id),
            pending: Vec::new(),
        });
        Visit::Children
    }

    fn enter_callable_decl(
        &mut self,
        node: Node,
        container: Option<SymbolId>,
        default_kind: SymbolKind,
    ) -> Visit {
        let mut annotations = self.take_pending();
        annotations.extend(self.direct_annotations(node));
        let Some(name_node) = first_child_kind(node, kinds::IDENT) else {
            self.push_frame(Mode::Body {
                container,
                pending: annotations,
            });
            return Visit::Children;
        };
        let kind = if default_kind == SymbolKind::Function && container.is_some() {
            SymbolKind::Method
        } else {
            default_kind
        };
        let type_annotation = direct_child_text(node, kinds::TYPE_ANNOT, self.source)
            .map(|t| Type::from_annotation(&t));
        let flavour = first_child_kind(node, kinds::FUNC_FLAVOUR)
            .and_then(|n| FuncFlavour::from_keyword(&node_text(n, self.source)));
        let (access, specifiers) = self.specifiers_of(node);
        let id = self.push_symbol(
            node,
            name_node,
            kind,
            SymbolSpec {
                container,
                annotations,
                type_annotation,
                flavour,
                access,
                specifiers,
                ..Default::default()
            },
        );

        // Parameters are pushed before block locals so SymbolId order stays func -> params -> locals.
        self.visit_params(node, id);
        self.push_frame(Mode::Callable {
            id,
            body_seen: false,
        });
        Visit::Children
    }

    fn visit_params(&mut self, node: Node, function_id: SymbolId) {
        if let Some(params) = first_child_kind(node, kinds::FUNC_PARAMS) {
            let mut cursor = params.walk();
            for group in params
                .children(&mut cursor)
                .filter(|child| child.kind() == kinds::FUNC_PARAM_GROUP)
            {
                self.visit_var_decl(group, Some(function_id), Vec::new(), SymbolKind::Parameter);
            }
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
        let type_annotation = direct_child_text(node, kinds::TYPE_ANNOT, self.source)
            .map(|t| Type::from_annotation(&t));
        let (access, specifiers) = self.specifiers_of(node);
        let mut cursor = node.walk();
        let names_field = if node.kind() == kinds::AUTOBIND_DECL {
            fields::NAME
        } else {
            fields::NAMES
        };

        for child in node.children_by_field_name(names_field, &mut cursor) {
            if child.kind() == kinds::IDENT {
                self.push_symbol(
                    node,
                    child,
                    kind,
                    SymbolSpec {
                        container,
                        annotations: annotations.clone(),
                        type_annotation: type_annotation.clone(),
                        access,
                        specifiers,
                        ..Default::default()
                    },
                );
            }
        }
    }

    fn annotation(&self, node: Node) -> Option<Annotation> {
        let name = first_child_kind(node, kinds::ANNOTATION_IDENT).map(|name| {
            node_text(name, self.source)
                .trim_start_matches('@')
                .to_string()
        })?;
        let argument = first_child_kind(node, kinds::IDENT).map(|arg| node_text(arg, self.source));

        Some(Annotation { name, argument })
    }

    fn direct_annotations(&self, node: Node) -> Vec<Annotation> {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .filter(|child| child.kind() == kinds::ANNOTATION)
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
            base_class: spec.base_class,
            owner_class: spec.owner_class,
            flavour: spec.flavour,
            annotations: spec.annotations,
            access: spec.access,
            specifiers: spec.specifiers,
        })
    }

    fn specifiers_of(&self, node: Node) -> (AccessLevel, Specifiers) {
        let mut access = AccessLevel::Public;
        let mut specifiers = Specifiers::default();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != kinds::SPECIFIER {
                continue;
            }
            match &self.source[child.start_byte()..child.end_byte()] {
                "private" => access = AccessLevel::Private,
                "protected" => access = AccessLevel::Protected,
                "public" => access = AccessLevel::Public,
                other => {
                    if let Some(specifier) = Specifier::from_keyword(other) {
                        specifiers.insert(specifier);
                    }
                }
            }
        }
        (access, specifiers)
    }
}
