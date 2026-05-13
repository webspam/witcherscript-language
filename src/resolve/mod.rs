use std::collections::HashMap;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::{SourcePosition, SourceRange};
use crate::script_env::ScriptEnvironment;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

#[derive(Debug, Clone)]
pub struct Definition {
    pub uri: String,
    pub symbol: Symbol,
}

pub struct SymbolDb<'a> {
    workspace: &'a WorkspaceIndex,
    base: &'a WorkspaceIndex,
    script_env: Option<&'a ScriptEnvironment>,
}

impl<'a> SymbolDb<'a> {
    pub fn new(workspace: &'a WorkspaceIndex, base: &'a WorkspaceIndex) -> Self {
        Self {
            workspace,
            base,
            script_env: None,
        }
    }

    pub fn with_script_env(mut self, env: &'a ScriptEnvironment) -> Self {
        self.script_env = Some(env);
        self
    }

    fn find_script_global(&self, name: &str) -> Option<Definition> {
        let g = self.script_env?.find(name)?;
        if let Some(class_def) = self.find_top_level(&g.type_name) {
            return Some(class_def);
        }
        Some(Definition {
            uri: g.ini_uri.clone(),
            symbol: g.symbol.clone(),
        })
    }

    fn script_global_type(&self, name: &str) -> Option<String> {
        self.script_env?.find(name).map(|g| g.type_name.clone())
    }

    pub fn find_top_level(&self, name: &str) -> Option<Definition> {
        self.workspace
            .find_top_level(name)
            .or_else(|| self.base.find_top_level(name))
    }

    pub fn find_member(
        &self,
        container: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        self.find_member_chain_cross(container, name, 0, min_access)
    }

    fn find_member_chain_cross(
        &self,
        container_name: &str,
        name: &str,
        depth: usize,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        if depth > 32 {
            return None;
        }
        let direct = self
            .workspace
            .direct_member_of(container_name, name, min_access)
            .or_else(|| self.base.direct_member_of(container_name, name, min_access));
        if direct.is_some() {
            return direct;
        }
        let superclass = self
            .workspace
            .superclass_of(container_name)
            .or_else(|| self.base.superclass_of(container_name))?;
        let deeper_min = min_access.max(AccessLevel::Protected);
        self.find_member_chain_cross(&superclass, name, depth + 1, deeper_min)
    }

    pub fn members_of(&self, container: &str, min_access: AccessLevel) -> Vec<Definition> {
        self.members_of_tiered(container, min_access)
            .into_iter()
            .map(|(_, def)| def)
            .collect()
    }

    pub fn members_of_tiered(
        &self,
        container: &str,
        min_access: AccessLevel,
    ) -> Vec<(u8, Definition)> {
        self.members_of_chain_cross(container, 0, min_access)
    }

    fn members_of_chain_cross(
        &self,
        container_name: &str,
        depth: usize,
        min_access: AccessLevel,
    ) -> Vec<(u8, Definition)> {
        if depth > 32 {
            return vec![];
        }
        let tier = if depth == 0 { 0u8 } else { 1u8 };
        let mut seen: HashMap<String, (u8, Definition)> = HashMap::new();
        for def in self
            .workspace
            .direct_members_of(container_name, min_access)
            .into_iter()
            .chain(self.base.direct_members_of(container_name, min_access))
        {
            seen.entry(def.symbol.name.clone()).or_insert((tier, def));
        }
        let superclass = self
            .workspace
            .superclass_of(container_name)
            .or_else(|| self.base.superclass_of(container_name));
        if let Some(superclass) = superclass {
            let deeper_min = min_access.max(AccessLevel::Protected);
            for item in self.members_of_chain_cross(&superclass, depth + 1, deeper_min) {
                seen.entry(item.1.symbol.name.clone()).or_insert(item);
            }
        }
        seen.into_values().collect()
    }

    pub fn all_types(&self) -> Vec<Definition> {
        let mut seen: HashMap<String, Definition> = HashMap::new();
        for def in self
            .workspace
            .all_types()
            .into_iter()
            .chain(self.base.all_types())
        {
            seen.entry(def.symbol.name.clone()).or_insert(def);
        }
        seen.into_values().collect()
    }

    pub fn all_top_level_callables(&self) -> Vec<Definition> {
        let mut seen: HashMap<String, Definition> = HashMap::new();
        for def in self
            .workspace
            .all_top_level_callables()
            .into_iter()
            .chain(self.base.all_top_level_callables())
        {
            seen.entry(def.symbol.name.clone()).or_insert(def);
        }
        seen.into_values().collect()
    }

    pub fn parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<String> {
        let params = self.workspace.parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        self.base.parameters_of(uri, callable_id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    documents: HashMap<String, Vec<Symbol>>,
    top_level_by_name: HashMap<String, Definition>,
    superclass_by_name: HashMap<String, String>,
    member_by_type: HashMap<String, HashMap<String, Definition>>,
    doc_idents: HashMap<String, HashMap<String, Vec<std::ops::Range<usize>>>>,
}

impl WorkspaceIndex {
    pub fn update_document(&mut self, uri: impl Into<String>, document: &ParsedDocument) {
        let uri: String = uri.into();
        self.remove_from_indices(&uri);
        self.doc_idents.remove(&uri);
        let all_symbols = document.symbols.all().to_vec();
        self.insert_into_indices(&uri, &all_symbols);
        self.doc_idents
            .insert(uri.clone(), scan_ident_occurrences(document));
        self.documents.insert(uri, all_symbols);
    }

    pub fn remove_document(&mut self, uri: &str) {
        self.remove_from_indices(uri);
        self.doc_idents.remove(uri);
        self.documents.remove(uri);
    }

    fn is_indexed(&self, uri: &str) -> bool {
        self.doc_idents.contains_key(uri)
    }

    /// Approximate heap bytes consumed by the ident occurrence index.
    pub fn doc_idents_bytes(&self) -> usize {
        let mut total = 0usize;
        for (uri, name_map) in &self.doc_idents {
            total += uri.capacity();
            for (name, ranges) in name_map {
                total += name.capacity();
                total += ranges.capacity() * size_of::<std::ops::Range<usize>>();
            }
            // HashMap slot overhead: ~56 bytes per entry (key ptr + value ptr + hash)
            total += name_map.capacity() * 56;
        }
        total += self.doc_idents.capacity() * 56;
        total
    }

    fn ident_ranges_in_doc(&self, uri: &str, name: &str) -> Option<&[std::ops::Range<usize>]> {
        self.doc_idents.get(uri)?.get(name).map(Vec::as_slice)
    }

    fn remove_from_indices(&mut self, uri: &str) {
        let Some(old_symbols) = self.documents.get(uri) else {
            return;
        };
        for sym in old_symbols.clone() {
            if sym.container.is_none() {
                if self
                    .top_level_by_name
                    .get(&sym.name)
                    .map(|d| d.uri == uri)
                    .unwrap_or(false)
                {
                    self.top_level_by_name.remove(&sym.name);
                }
                if is_type_like(sym.kind) {
                    self.superclass_by_name.remove(&sym.name);
                }
            } else if let Some(cn) = &sym.container_name {
                if let Some(members) = self.member_by_type.get_mut(cn) {
                    if members
                        .get(&sym.name)
                        .map(|d| d.uri == uri)
                        .unwrap_or(false)
                    {
                        members.remove(&sym.name);
                    }
                    if members.is_empty() {
                        self.member_by_type.remove(cn);
                    }
                }
            }
        }
    }

    fn insert_into_indices(&mut self, uri: &str, symbols: &[Symbol]) {
        for sym in symbols {
            if sym.container.is_none() {
                self.top_level_by_name.insert(
                    sym.name.clone(),
                    Definition {
                        uri: uri.to_string(),
                        symbol: sym.clone(),
                    },
                );
                if is_type_like(sym.kind) {
                    if let Some(superclass) = &sym.base_class {
                        self.superclass_by_name
                            .insert(sym.name.clone(), superclass.clone());
                    }
                }
            } else if let Some(cn) = &sym.container_name {
                self.member_by_type.entry(cn.clone()).or_default().insert(
                    sym.name.clone(),
                    Definition {
                        uri: uri.to_string(),
                        symbol: sym.clone(),
                    },
                );
            }
        }
    }

    pub fn find_top_level(&self, name: &str) -> Option<Definition> {
        self.top_level_by_name.get(name).cloned()
    }

    pub fn all_types(&self) -> Vec<Definition> {
        self.top_level_by_name
            .values()
            .filter(|d| is_type_like(d.symbol.kind) || d.symbol.kind == SymbolKind::Enum)
            .cloned()
            .collect()
    }

    pub fn all_top_level_callables(&self) -> Vec<Definition> {
        self.top_level_by_name
            .values()
            .filter(|d| {
                matches!(d.symbol.kind, SymbolKind::Function | SymbolKind::Event)
                    && !matches!(d.symbol.flavour.as_deref(), Some("exec") | Some("quest"))
            })
            .cloned()
            .collect()
    }

    pub fn find_member(
        &self,
        container_name: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        self.find_member_in_chain(container_name, name, 0, min_access)
    }

    pub fn direct_member_of(
        &self,
        container_name: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        self.member_by_type
            .get(container_name)
            .and_then(|members| members.get(name))
            .filter(|def| def.symbol.access >= min_access)
            .cloned()
    }

    pub fn direct_members_of(
        &self,
        container_name: &str,
        min_access: AccessLevel,
    ) -> Vec<Definition> {
        self.member_by_type
            .get(container_name)
            .map(|m| {
                m.values()
                    .filter(|d| d.symbol.access >= min_access)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.superclass_by_name.get(class_name).cloned()
    }

    pub fn members_of(&self, container_name: &str, min_access: AccessLevel) -> Vec<Definition> {
        self.members_of_chain(container_name, 0, min_access)
    }

    pub fn parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<String> {
        let Some(symbols) = self.documents.get(uri) else {
            return vec![];
        };
        symbols
            .iter()
            .filter(|s| {
                s.kind == SymbolKind::Parameter
                    && s.container == Some(callable_id)
                    && !s.is_optional
            })
            .map(|s| s.name.clone())
            .collect()
    }

    fn members_of_chain(
        &self,
        container_name: &str,
        depth: usize,
        min_access: AccessLevel,
    ) -> Vec<Definition> {
        if depth > 32 {
            return vec![];
        }
        let mut result: Vec<Definition> = self
            .member_by_type
            .get(container_name)
            .map(|m| {
                m.values()
                    .filter(|d| d.symbol.access >= min_access)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if let Some(superclass) = self.superclass_by_name.get(container_name).cloned() {
            let deeper_min = min_access.max(AccessLevel::Protected);
            for def in self.members_of_chain(&superclass, depth + 1, deeper_min) {
                if !result.iter().any(|d| d.symbol.name == def.symbol.name) {
                    result.push(def);
                }
            }
        }
        result
    }

    fn find_member_in_chain(
        &self,
        container_name: &str,
        name: &str,
        depth: usize,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        if depth > 32 {
            return None;
        }
        let direct = self
            .member_by_type
            .get(container_name)
            .and_then(|members| members.get(name))
            .filter(|def| def.symbol.access >= min_access)
            .cloned();
        if direct.is_some() {
            return direct;
        }
        let superclass = self.superclass_by_name.get(container_name)?.clone();
        let deeper_min = min_access.max(AccessLevel::Protected);
        self.find_member_in_chain(&superclass, name, depth + 1, deeper_min)
    }
}

pub fn resolve_definition(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Definition> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    if let Some(def) = resolve_self_keyword(uri, document, db, byte_offset) {
        return Some(def);
    }

    let ident = identifier_at(document.tree.root_node(), byte_offset)?;
    let name = ident.utf8_text(document.source.as_bytes()).ok()?;

    if let Some(member_access) = ident.parent().filter(|p| p.kind() == "member_access_expr") {
        let is_receiver = first_named_child(member_access)
            .map(|r| r.id() == ident.id())
            .unwrap_or(false);
        if !is_receiver {
            return resolve_member_access(uri, document, db, ident, name);
        }
    }

    resolve_local_or_parameter(uri, document, byte_offset, name)
        .or_else(|| resolve_current_type_member(uri, document, db, byte_offset, name))
        .or_else(|| resolve_document_top_level(uri, document, name))
        .or_else(|| db.find_top_level(name))
        .or_else(|| db.find_script_global(name))
        .or_else(|| resolve_at_definition_site(uri, document, byte_offset, name))
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
            let params_and_return = symbol
                .signature
                .as_deref()
                .and_then(|sig| sig.find('(').map(|i| &sig[i..]))
                .unwrap_or("");
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
                SymbolKind::EnumVariant => "enum variant",
                SymbolKind::Function => "function",
                SymbolKind::Variable => "var",
                SymbolKind::Parameter => "(parameter)",
                SymbolKind::State => "state",
                SymbolKind::Event => "event",
                SymbolKind::Method | SymbolKind::Field => unreachable!(),
            };
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
        }
    }

    lines.join("\n")
}

fn infer_expr_type(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
) -> Option<String> {
    match node.kind() {
        "ident" => {
            let name = node.utf8_text(document.source.as_bytes()).ok()?;
            resolve_local_or_parameter(uri, document, context_byte, name)
                .or_else(|| {
                    let current_type = current_type_name(document, context_byte)?;
                    resolve_document_member(
                        uri,
                        document,
                        &current_type,
                        name,
                        AccessLevel::Private,
                    )
                    .or_else(|| db.find_member(&current_type, name, AccessLevel::Private))
                })
                .or_else(|| resolve_document_top_level(uri, document, name))
                .or_else(|| db.find_top_level(name))
                .and_then(|def| def.symbol.type_annotation)
                .or_else(|| db.script_global_type(name))
        }
        "func_call_expr" => {
            let func = node
                .child_by_field_name("func")
                .or_else(|| first_named_child(node))?;
            infer_expr_type(uri, document, db, func, context_byte)
        }
        "member_access_expr" => {
            let accessor = first_named_child(node)?;
            let member = node.child_by_field_name("member").or_else(|| {
                let mut cursor = node.walk();
                let child = node.named_children(&mut cursor).nth(1);
                child
            })?;
            if member.kind() != "ident" {
                return None;
            }
            let member_name = member.utf8_text(document.source.as_bytes()).ok()?;
            let container_type = infer_expr_type(uri, document, db, accessor, context_byte)?;
            let def = resolve_document_member(
                uri,
                document,
                &container_type,
                member_name,
                AccessLevel::Public,
            )
            .or_else(|| db.find_member(&container_type, member_name, AccessLevel::Public))?;
            def.symbol.type_annotation
        }
        "this_expr" => current_type_name(document, context_byte),
        _ => None,
    }
}

fn resolve_member_access(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
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
            resolve_document_member(uri, document, &current_type, name, AccessLevel::Private)
                .or_else(|| db.find_member(&current_type, name, AccessLevel::Private))
        }
        "super_expr" => {
            let current_type = current_type_symbol(document, ident.start_byte())?;
            db.find_member(
                current_type.base_class.as_deref()?,
                name,
                AccessLevel::Protected,
            )
        }
        "parent_expr" => {
            let current_type = current_type_symbol(document, ident.start_byte())?;
            db.find_member(
                current_type.owner_class.as_deref()?,
                name,
                AccessLevel::Public,
            )
        }
        "ident" => {
            let receiver_name = receiver.utf8_text(document.source.as_bytes()).ok()?;
            let type_name =
                resolve_local_or_parameter(uri, document, ident.start_byte(), receiver_name)
                    .or_else(|| {
                        let current_type = current_type_name(document, ident.start_byte())?;
                        resolve_document_member(
                            uri,
                            document,
                            &current_type,
                            receiver_name,
                            AccessLevel::Private,
                        )
                        .or_else(|| {
                            db.find_member(&current_type, receiver_name, AccessLevel::Private)
                        })
                    })
                    .and_then(|def| def.symbol.type_annotation)
                    .or_else(|| db.script_global_type(receiver_name))?;
            resolve_document_member(uri, document, &type_name, name, AccessLevel::Public)
                .or_else(|| db.find_member(&type_name, name, AccessLevel::Public))
        }
        "func_call_expr" | "member_access_expr" => {
            let type_name = infer_expr_type(uri, document, db, receiver, ident.start_byte())?;
            resolve_document_member(uri, document, &type_name, name, AccessLevel::Public)
                .or_else(|| db.find_member(&type_name, name, AccessLevel::Public))
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
    db: &SymbolDb,
    byte_offset: usize,
    name: &str,
) -> Option<Definition> {
    let current_type = current_type_name(document, byte_offset)?;
    resolve_document_member(uri, document, &current_type, name, AccessLevel::Private)
        .or_else(|| db.find_member(&current_type, name, AccessLevel::Private))
}

fn resolve_document_member(
    uri: &str,
    document: &ParsedDocument,
    container_name: &str,
    name: &str,
    min_access: AccessLevel,
) -> Option<Definition> {
    let container = document
        .symbols
        .all()
        .iter()
        .find(|symbol| symbol.name == container_name && is_type_like(symbol.kind))?;
    document
        .symbols
        .children_of(Some(container.id))
        .find(|symbol| symbol.name == name && symbol.access >= min_access)
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
    nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|node| {
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
        })
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

enum SearchScope {
    AllDocuments,
    SingleFile,
    SingleFileRange(std::ops::Range<usize>),
}

fn definition_search_scope(
    definition: &Definition,
    definition_document: &ParsedDocument,
) -> SearchScope {
    let container_range = || {
        definition
            .symbol
            .container
            .and_then(|id| definition_document.symbols.by_id(id))
            .map(|container| container.byte_range.clone())
    };

    match definition.symbol.kind {
        SymbolKind::Variable | SymbolKind::Parameter => match container_range() {
            Some(r) => SearchScope::SingleFileRange(r),
            None => SearchScope::SingleFile,
        },
        SymbolKind::Method | SymbolKind::Field
            if definition.symbol.access == AccessLevel::Private =>
        {
            match container_range() {
                Some(r) => SearchScope::SingleFileRange(r),
                None => SearchScope::SingleFile,
            }
        }
        _ => SearchScope::AllDocuments,
    }
}

pub fn find_references(
    definition: &Definition,
    definition_document: &ParsedDocument,
    search_documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
    include_declaration: bool,
) -> Vec<(String, SourceRange)> {
    let name = &definition.symbol.name;
    let scope = definition_search_scope(definition, definition_document);

    let mut results = Vec::new();

    for (uri, document) in search_documents {
        let scan_range: Option<&std::ops::Range<usize>> = match &scope {
            SearchScope::AllDocuments => None,
            SearchScope::SingleFile => {
                if *uri != definition.uri.as_str() {
                    continue;
                }
                None
            }
            SearchScope::SingleFileRange(r) => {
                if *uri != definition.uri.as_str() {
                    continue;
                }
                Some(r)
            }
        };

        let mut byte_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        if scan_range.is_none() {
            if db.workspace.is_indexed(uri) || db.base.is_indexed(uri) {
                // Document is in the index: use it. If the name isn't present,
                // both calls return None and byte_ranges stays empty — no tree scan.
                if let Some(ranges) = db
                    .workspace
                    .ident_ranges_in_doc(uri, name)
                    .or_else(|| db.base.ident_ranges_in_doc(uri, name))
                {
                    byte_ranges.extend_from_slice(ranges);
                }
            } else {
                collect_ident_occurrences(
                    document.tree.root_node(),
                    document.source.as_bytes(),
                    name,
                    None,
                    &mut byte_ranges,
                );
            }
        } else {
            collect_ident_occurrences(
                document.tree.root_node(),
                document.source.as_bytes(),
                name,
                scan_range,
                &mut byte_ranges,
            );
        }

        for byte_range in byte_ranges {
            // Semantic verification: resolve the candidate and confirm it points
            // at the same definition (same file + same selection range).
            let position = document
                .line_index
                .byte_to_position(&document.source, byte_range.start);
            let resolved = resolve_definition(uri, document, db, position);
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

fn scan_ident_occurrences(
    document: &ParsedDocument,
) -> HashMap<String, Vec<std::ops::Range<usize>>> {
    let mut map: HashMap<String, Vec<std::ops::Range<usize>>> = HashMap::new();
    collect_all_idents(
        document.tree.root_node(),
        document.source.as_bytes(),
        &mut map,
    );
    map
}

fn collect_all_idents<'tree>(
    node: Node<'tree>,
    source: &[u8],
    map: &mut HashMap<String, Vec<std::ops::Range<usize>>>,
) {
    if node.kind() == "ident" {
        if let Ok(name) = node.utf8_text(source) {
            map.entry(name.to_string())
                .or_default()
                .push(node.start_byte()..node.end_byte());
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_all_idents(child, source, map);
    }
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

fn resolve_self_keyword(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<Definition> {
    let root = document.tree.root_node();
    let node = nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|n| find_ancestor_of_kind(n, &["this_expr", "super_expr", "parent_expr"]))?;

    match node.kind() {
        "this_expr" => {
            let current_type = current_type_symbol(document, byte_offset)?;
            resolve_document_top_level(uri, document, &current_type.name.clone())
                .or_else(|| db.find_top_level(&current_type.name))
        }
        "super_expr" => {
            let current_type = current_type_symbol(document, byte_offset)?;
            let base_name = current_type.base_class.as_deref()?;
            resolve_document_top_level(uri, document, base_name)
                .or_else(|| db.find_top_level(base_name))
        }
        "parent_expr" => {
            let current_type = current_type_symbol(document, byte_offset)?;
            let owner_name = current_type.owner_class.as_deref()?;
            resolve_document_top_level(uri, document, owner_name)
                .or_else(|| db.find_top_level(owner_name))
        }
        _ => None,
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

pub fn completion_members(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<(u8, Definition)> {
    completion_members_inner(uri, document, db, position).unwrap_or_default()
}

fn completion_members_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<(u8, Definition)>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let access_node = nodes_at_offset(root, byte_offset)
        .into_iter()
        .find_map(|n| {
            find_ancestor_of_kind(n, &["member_access_expr", "incomplete_member_access_expr"])
        })?;

    let expr = first_named_child(access_node)?;
    let context_byte = expr.start_byte();

    let type_name = match expr.kind() {
        "super_expr" | "super" => {
            let current_type = current_type_symbol(document, context_byte)?;
            current_type.base_class.as_deref()?.to_string()
        }
        "parent_expr" | "parent" => {
            let current_type = current_type_symbol(document, context_byte)?;
            current_type.owner_class.as_deref()?.to_string()
        }
        _ => infer_expr_type(uri, document, db, expr, context_byte)?,
    };

    Some(db.members_of_tiered(&type_name, AccessLevel::Public))
}

fn find_ancestor_of_kind<'a>(mut node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    loop {
        if kinds.contains(&node.kind()) {
            return Some(node);
        }
        node = node.parent()?;
    }
}

fn nodes_at_offset<'a>(root: Node<'a>, byte_offset: usize) -> Vec<Node<'a>> {
    let second = byte_offset.checked_sub(1);
    [Some(byte_offset), second]
        .into_iter()
        .flatten()
        .filter_map(|off| root.descendant_for_byte_range(off, off))
        .collect()
}

pub const BUILTIN_TYPES: &[&str] = &["bool", "byte", "float", "int", "name", "string", "void"];

pub fn type_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    type_completions_inner(document, db, position).unwrap_or_default()
}

fn type_completions_inner(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<Definition>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let in_type_annot = nodes_at_offset(root, byte_offset)
        .into_iter()
        .any(has_type_annot_ancestor);

    if !in_type_annot {
        return None;
    }

    Some(db.all_types())
}

fn has_type_annot_ancestor(node: Node) -> bool {
    let mut current = node;
    loop {
        if current.kind() == "type_annot" {
            return true;
        }
        match current.parent() {
            Some(p) => current = p,
            None => return false,
        }
    }
}

pub fn extends_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    extends_completions_inner(document, db, position).unwrap_or_default()
}

fn extends_completions_inner(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<Definition>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();

    // When the cursor sits in trailing whitespace (after the last token of an incomplete
    // declaration), descendant_for_byte_range returns the root "script" node because
    // whitespace is transparent in the AST. Filter those out, then fall back to finding
    // the last top-level child whose byte range ends at or before the cursor — that child
    // is the context we want to inspect.
    let direct: Vec<Node> = nodes_at_offset(root, byte_offset)
        .into_iter()
        .filter(|n| n.kind() != "script")
        .collect();

    let in_extends = if !direct.is_empty() {
        direct
            .iter()
            .any(|n| in_class_extends_position(*n, byte_offset))
    } else {
        let mut tc = root.walk();
        root.children(&mut tc)
            .take_while(|c| c.end_byte() <= byte_offset)
            .last()
            .is_some_and(|n| in_class_extends_position(n, byte_offset))
    };

    if !in_extends {
        return None;
    }

    Some(
        db.all_types()
            .into_iter()
            .filter(|def| matches!(def.symbol.kind, SymbolKind::Class | SymbolKind::State))
            .collect(),
    )
}

fn in_class_extends_position(node: Node, byte_offset: usize) -> bool {
    let mut current = node;
    loop {
        match current.kind() {
            "class_decl" | "state_decl" => {
                return is_after_extends_before_body(current, byte_offset);
            }
            "ERROR" => {
                if let Some(parent) = current.parent() {
                    if matches!(parent.kind(), "class_decl" | "state_decl") {
                        return is_after_extends_before_body(parent, byte_offset);
                    }
                }
                return error_node_has_class_extends(current, byte_offset);
            }
            _ => {}
        }
        match current.parent() {
            Some(p) => current = p,
            None => return false,
        }
    }
}

fn is_after_extends_before_body(decl_node: Node, byte_offset: usize) -> bool {
    let mut cursor = decl_node.walk();
    let mut saw_extends = false;
    for child in decl_node.children(&mut cursor) {
        if child.start_byte() >= byte_offset {
            break;
        }
        match child.kind() {
            "extends" => saw_extends = true,
            "class_def" => return false,
            // When _class_base fails (extends without a following ident), tree-sitter
            // wraps the stranded 'extends' keyword in an ERROR child of the decl node.
            // Scan one level into that ERROR to detect the keyword.
            "ERROR" if node_contains_kind(child, "extends") => {
                saw_extends = true;
            }
            _ => {}
        }
    }
    saw_extends
}

fn node_contains_kind(node: Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| c.kind() == kind);
    found
}

fn error_node_has_class_extends(error_node: Node, byte_offset: usize) -> bool {
    let mut cursor = error_node.walk();
    let mut saw_class_kw = false;
    let mut saw_extends = false;
    for child in error_node.children(&mut cursor) {
        if child.start_byte() >= byte_offset {
            break;
        }
        match child.kind() {
            "class" | "state" => saw_class_kw = true,
            "extends" if saw_class_kw => {
                saw_extends = true;
            }
            "{" => return false,
            _ => {}
        }
    }
    saw_extends
}

pub struct StatementCompletions {
    pub locals: Vec<Definition>,
    pub members: Vec<Definition>,
    pub globals: Vec<Definition>,
    pub has_this: bool,
    pub has_super: bool,
}

pub fn statement_completions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> StatementCompletions {
    statement_completions_inner(uri, document, db, position).unwrap_or(StatementCompletions {
        locals: vec![],
        members: vec![],
        globals: vec![],
        has_this: false,
        has_super: false,
    })
}

fn statement_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<StatementCompletions> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);
    if nodes.iter().any(|n| {
        find_ancestor_of_kind(
            *n,
            &[
                "member_access_expr",
                "incomplete_member_access_expr",
                "ERROR",
            ],
        )
        .is_some()
    }) {
        return None;
    }

    let in_func_body = nodes
        .iter()
        .any(|n| find_ancestor_of_kind(*n, &["func_block"]).is_some());

    if !in_func_body {
        return None;
    }

    let callable = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    )?;

    let locals: Vec<Definition> = document
        .symbols
        .children_of(Some(callable.id))
        .filter(|sym| {
            matches!(sym.kind, SymbolKind::Variable | SymbolKind::Parameter)
                && sym.selection_byte_range.start <= byte_offset
        })
        .cloned()
        .map(|symbol| Definition {
            uri: uri.to_string(),
            symbol,
        })
        .collect();

    let current_type = current_type_symbol(document, byte_offset);

    let members: Vec<Definition> = current_type
        .map(|t| db.members_of(&t.name, AccessLevel::Private))
        .unwrap_or_default();

    let has_this = current_type.is_some();
    let has_super = current_type.and_then(|t| t.base_class.as_deref()).is_some();

    let globals = db.all_top_level_callables();

    Some(StatementCompletions {
        locals,
        members,
        globals,
        has_this,
        has_super,
    })
}

#[allow(dead_code)]
fn symbol_id(symbol: &Symbol) -> SymbolId {
    symbol.id
}

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
    let ctx = scan_class_body_ctx(&document.source, byte_offset, kind);

    if ctx.saw_decl_keyword {
        return None;
    }

    Some(class_body_kw_candidates(&ctx))
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

fn class_body_stmt_start(source: &str, byte_offset: usize) -> usize {
    let before = &source[..byte_offset.min(source.len())];
    let bytes = before.as_bytes();
    let mut depth = 0i32;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'}' => depth += 1,
            b'{' if depth > 0 => depth -= 1,
            b'{' | b';' if depth == 0 => return i + 1,
            _ => {}
        }
    }
    0
}

fn scan_class_body_ctx(source: &str, byte_offset: usize, kind: ClassBodyKind) -> ClassBodyCtx {
    let stmt_start = class_body_stmt_start(source, byte_offset);
    let stmt_text = source[stmt_start..byte_offset.min(source.len())].trim_start();

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

    for token in stmt_text.split_ascii_whitespace() {
        match token {
            "private" | "protected" | "public" => ctx.has_access = true,
            "import" => ctx.has_import = true,
            "final" => ctx.has_final = true,
            "latent" => ctx.has_latent = true,
            "editable" => ctx.has_editable = true,
            "saved" => ctx.has_saved = true,
            "const" => ctx.has_const_ = true,
            "inlined" => ctx.has_inlined = true,
            "optional" => ctx.has_optional = true,
            "var" | "function" | "event" | "autobind" | "default" | "defaults" | "hint" => {
                ctx.saw_decl_keyword = true;
                break;
            }
            _ => {}
        }
    }

    ctx
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

#[cfg(test)]
mod tests;
