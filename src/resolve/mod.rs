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
        self.workspace
            .find_member(container, name, min_access)
            .or_else(|| self.base.find_member(container, name, min_access))
    }

    pub fn members_of(&self, container: &str, min_access: AccessLevel) -> Vec<Definition> {
        let mut seen: HashMap<String, Definition> = HashMap::new();
        for def in self
            .workspace
            .members_of(container, min_access)
            .into_iter()
            .chain(self.base.members_of(container, min_access))
        {
            seen.entry(def.symbol.name.clone()).or_insert(def);
        }
        seen.into_values().collect()
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
                    if let Some(superclass) = sym
                        .detail
                        .as_deref()
                        .and_then(|d| d.strip_prefix("extends "))
                    {
                        self.superclass_by_name
                            .insert(sym.name.clone(), superclass.to_string());
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

    pub fn find_member(
        &self,
        container_name: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        self.find_member_in_chain(container_name, name, 0, min_access)
    }

    pub fn members_of(&self, container_name: &str, min_access: AccessLevel) -> Vec<Definition> {
        self.members_of_chain(container_name, 0, min_access)
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
            let base_name = current_type.detail.as_deref()?.strip_prefix("extends ")?;
            db.find_member(base_name, name, AccessLevel::Protected)
        }
        "parent_expr" => {
            let current_type = current_type_symbol(document, ident.start_byte())?;
            let owner_name = current_type.detail.as_deref()?.strip_prefix("in ")?;
            db.find_member(owner_name, name, AccessLevel::Public)
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
    let node = [byte_offset, byte_offset.saturating_sub(1)]
        .into_iter()
        .find_map(|offset| root.descendant_for_byte_range(offset, offset))?;

    let parent_kind = node.parent().map(|p| p.kind());

    let is_this = node.kind() == "this" || matches!(parent_kind, Some("this_expr"));
    let is_super = node.kind() == "super" || matches!(parent_kind, Some("super_expr"));
    let is_parent = node.kind() == "parent" || matches!(parent_kind, Some("parent_expr"));

    if is_this {
        let current_type = current_type_symbol(document, byte_offset)?;
        return resolve_document_top_level(uri, document, &current_type.name.clone())
            .or_else(|| db.find_top_level(&current_type.name));
    }

    if is_super {
        let current_type = current_type_symbol(document, byte_offset)?;
        let base_name = current_type
            .detail
            .as_deref()
            .and_then(|d| d.strip_prefix("extends "))?;
        return resolve_document_top_level(uri, document, base_name)
            .or_else(|| db.find_top_level(base_name));
    }

    if is_parent {
        let current_type = current_type_symbol(document, byte_offset)?;
        let owner_name = current_type
            .detail
            .as_deref()
            .and_then(|d| d.strip_prefix("in "))?;
        return resolve_document_top_level(uri, document, owner_name)
            .or_else(|| db.find_top_level(owner_name));
    }

    None
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
) -> Vec<Definition> {
    completion_members_inner(uri, document, db, position).unwrap_or_default()
}

fn completion_members_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<Definition>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let src = document.source.as_bytes();

    // Skip back over ident chars (cursor may be inside a partial member name).
    let mut scan = byte_offset;
    while scan > 0 && is_ident_byte(src[scan - 1]) {
        scan -= 1;
    }

    // The character immediately before the (possibly partial) member name must be '.'.
    let dot_byte = scan.checked_sub(1)?;
    if src.get(dot_byte) != Some(&b'.') {
        return None;
    }

    let before_dot = dot_byte.checked_sub(1)?;
    let root = document.tree.root_node();
    let node = root.descendant_for_byte_range(before_dot, before_dot)?;
    let expr = climb_to_expression(node);

    let type_name = match expr.kind() {
        "super_expr" | "super" => {
            let current_type = current_type_symbol(document, before_dot)?;
            current_type
                .detail
                .as_deref()?
                .strip_prefix("extends ")?
                .to_string()
        }
        "parent_expr" | "parent" => {
            let current_type = current_type_symbol(document, before_dot)?;
            current_type
                .detail
                .as_deref()?
                .strip_prefix("in ")?
                .to_string()
        }
        _ => infer_expr_type(uri, document, db, expr, before_dot)?,
    };

    Some(db.members_of(&type_name, AccessLevel::Public))
}

fn climb_to_expression(node: Node) -> Node {
    const EXPR_KINDS: &[&str] = &[
        "ident",
        "this_expr",
        "super_expr",
        "parent_expr",
        "func_call_expr",
        "member_access_expr",
    ];
    let mut current = node;
    loop {
        if EXPR_KINDS.contains(&current.kind()) {
            return current;
        }
        match current.parent() {
            Some(p) => current = p,
            None => return node,
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[allow(dead_code)]
fn symbol_id(symbol: &Symbol) -> SymbolId {
    symbol.id
}

#[cfg(test)]
mod tests;
