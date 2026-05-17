use std::collections::HashMap;

use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::line_index::{SourcePosition, SourceRange};
use crate::script_env::ScriptEnvironment;
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

// AGENTS.md key invariant #3.
const MAX_INHERITANCE_DEPTH: usize = 32;

#[derive(Debug, Clone)]
pub struct Definition {
    pub uri: String,
    pub symbol: Symbol,
}

#[derive(Debug, Clone)]
struct TypeContext {
    name: String,
    base_class: Option<String>,
    owner_class: Option<String>,
}

const METHOD_INJECTING_ANNOTATIONS: &[&str] = &["addMethod", "wrapMethod", "replaceMethod"];

const MODDING_ANNOTATIONS: &[&str] = &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"];

fn annotation_target_class(symbol: &Symbol) -> Option<&str> {
    symbol
        .annotations
        .iter()
        .find(|a| METHOD_INJECTING_ANNOTATIONS.contains(&a.name.as_str()))
        .and_then(|a| a.argument.as_deref())
}

pub struct SymbolDb<'a> {
    workspace: &'a WorkspaceIndex,
    base: &'a WorkspaceIndex,
    builtins: Option<&'a WorkspaceIndex>,
    script_env: Option<&'a ScriptEnvironment>,
}

impl<'a> SymbolDb<'a> {
    pub fn new(workspace: &'a WorkspaceIndex, base: &'a WorkspaceIndex) -> Self {
        Self {
            workspace,
            base,
            builtins: None,
            script_env: None,
        }
    }

    pub fn with_script_env(mut self, env: &'a ScriptEnvironment) -> Self {
        self.script_env = Some(env);
        self
    }

    pub fn with_builtins(mut self, builtins: &'a WorkspaceIndex) -> Self {
        self.builtins = Some(builtins);
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
            .or_else(|| self.builtins.and_then(|b| b.find_top_level(name)))
    }

    pub fn find_enum_variant(&self, name: &str) -> Option<Definition> {
        self.workspace
            .find_enum_variant(name)
            .or_else(|| self.base.find_enum_variant(name))
            .or_else(|| self.builtins.and_then(|b| b.find_enum_variant(name)))
    }

    pub fn all_enum_variants(&self) -> Vec<Definition> {
        dedup_by_name(
            self.workspace
                .all_enum_variants()
                .into_iter()
                .chain(self.base.all_enum_variants()),
        )
    }

    pub fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.workspace
            .superclass_of(class_name)
            .or_else(|| self.base.superclass_of(class_name))
            .or_else(|| self.builtins.and_then(|b| b.superclass_of(class_name)))
    }

    pub fn find_member(
        &self,
        container: &str,
        name: &str,
        min_access: AccessLevel,
    ) -> Option<Definition> {
        let (lookup, element) = generic_lookup_target(container);
        let def = self.try_in_chain(lookup, min_access, |container, _depth, access| {
            self.workspace
                .direct_member_of(container, name, access)
                .or_else(|| self.base.direct_member_of(container, name, access))
                .or_else(|| {
                    self.builtins
                        .and_then(|b| b.direct_member_of(container, name, access))
                })
        });
        match (def, element) {
            (Some(d), Some(elem)) => Some(substitute_in_definition(d, container, elem)),
            (d, _) => d,
        }
    }

    pub fn direct_members_of(
        &self,
        container_name: &str,
        min_access: AccessLevel,
    ) -> Vec<Definition> {
        let (lookup, element) = generic_lookup_target(container_name);
        let raw = dedup_by_name(
            self.workspace
                .direct_members_of(lookup, min_access)
                .into_iter()
                .chain(self.base.direct_members_of(lookup, min_access))
                .chain(
                    self.builtins
                        .map(|b| b.direct_members_of(lookup, min_access))
                        .unwrap_or_default(),
                ),
        );
        match element {
            Some(elem) => raw
                .into_iter()
                .map(|d| substitute_in_definition(d, container_name, elem))
                .collect(),
            None => raw,
        }
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
        let (lookup, element) = generic_lookup_target(container);
        let mut seen: HashMap<String, (u8, Definition)> = HashMap::new();
        self.try_in_chain::<(), _>(lookup, min_access, |c, depth, access| {
            let tier = if depth == 0 { 0u8 } else { 1u8 };
            for def in self
                .workspace
                .direct_members_of(c, access)
                .into_iter()
                .chain(self.base.direct_members_of(c, access))
                .chain(
                    self.builtins
                        .map(|b| b.direct_members_of(c, access))
                        .unwrap_or_default(),
                )
            {
                seen.entry(def.symbol.name.clone()).or_insert((tier, def));
            }
            None
        });
        match element {
            Some(elem) => seen
                .into_values()
                .map(|(t, d)| (t, substitute_in_definition(d, container, elem)))
                .collect(),
            None => seen.into_values().collect(),
        }
    }

    /// Class-body declaration first, then annotation declarations.
    fn all_member_declarations(&self, container: &str, name: &str) -> Vec<Definition> {
        let mut decls: Vec<Definition> = Vec::new();
        if let Some(class_body) = self.find_member(container, name, AccessLevel::Private) {
            decls.push(class_body);
        }
        for def in self
            .workspace
            .annotated_members(container, name)
            .into_iter()
            .chain(self.base.annotated_members(container, name))
            .chain(
                self.builtins
                    .map(|b| b.annotated_members(container, name))
                    .unwrap_or_default(),
            )
        {
            decls.push(def);
        }
        dedup_definitions(decls)
    }

    fn try_in_chain<T, F>(&self, start: &str, min_access: AccessLevel, mut visit: F) -> Option<T>
    where
        F: FnMut(&str, usize, AccessLevel) -> Option<T>,
    {
        let mut current: String = start.to_string();
        let mut depth: usize = 0;
        let mut access = min_access;
        loop {
            if depth > MAX_INHERITANCE_DEPTH {
                return None;
            }
            if let Some(found) = visit(&current, depth, access) {
                return Some(found);
            }
            let superclass = self
                .workspace
                .superclass_of(&current)
                .or_else(|| self.base.superclass_of(&current))
                .or_else(|| self.builtins.and_then(|b| b.superclass_of(&current)))?;
            depth += 1;
            access = access.max(AccessLevel::Protected);
            current = superclass;
        }
    }

    pub fn all_types(&self) -> Vec<Definition> {
        dedup_by_name(
            self.workspace
                .all_types()
                .into_iter()
                .chain(self.base.all_types()),
        )
    }

    pub fn all_top_level_callables(&self) -> Vec<Definition> {
        dedup_by_name(
            self.workspace
                .all_top_level_callables()
                .into_iter()
                .chain(self.base.all_top_level_callables()),
        )
    }

    pub fn all_script_globals(&self) -> Vec<Definition> {
        self.script_env
            .map(|env| {
                env.globals
                    .iter()
                    .map(|g| Definition {
                        uri: g.ini_uri.clone(),
                        symbol: g.symbol.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<String> {
        let params = self.workspace.parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        let params = self.base.parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        self.builtins
            .map(|b| b.parameters_of(uri, callable_id))
            .unwrap_or_default()
    }

    pub fn full_parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<Symbol> {
        let params = self.workspace.full_parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        let params = self.base.full_parameters_of(uri, callable_id);
        if !params.is_empty() {
            return params;
        }
        self.builtins
            .map(|b| b.full_parameters_of(uri, callable_id))
            .unwrap_or_default()
    }
}

pub fn parse_generic_type(s: &str) -> Option<(&str, &str)> {
    let trimmed = s.trim();
    let lt = trimmed.find('<')?;
    if !trimmed.ends_with('>') {
        return None;
    }
    let ctor = trimmed[..lt].trim();
    let element = trimmed[lt + 1..trimmed.len() - 1].trim();
    if ctor.is_empty() || element.is_empty() {
        return None;
    }
    Some((ctor, element))
}

fn generic_lookup_target(container: &str) -> (&str, Option<&str>) {
    match parse_generic_type(container) {
        Some((ctor, elem)) => (ctor, Some(elem)),
        None => (container, None),
    }
}

fn substitute_placeholder(s: &str, placeholder: &str, replacement: &str) -> String {
    let bytes = s.as_bytes();
    let plen = placeholder.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(placeholder.as_bytes()) {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_idx = i + plen;
            let after_ok = after_idx >= bytes.len() || !is_ident_byte(bytes[after_idx]);
            if before_ok && after_ok {
                out.push_str(replacement);
                i += plen;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn substitute_in_definition(
    mut def: Definition,
    container_instance: &str,
    element: &str,
) -> Definition {
    let p = crate::builtins::GENERIC_ELEMENT_PLACEHOLDER;
    if let Some(t) = def.symbol.type_annotation.take() {
        def.symbol.type_annotation = Some(substitute_placeholder(&t, p, element));
    }
    if let Some(s) = def.symbol.signature.take() {
        def.symbol.signature = Some(substitute_placeholder(&s, p, element));
    }
    if let Some(d) = def.symbol.detail.take() {
        def.symbol.detail = Some(substitute_placeholder(&d, p, element));
    }
    if def.symbol.container_name.is_some() {
        def.symbol.container_name = Some(container_instance.to_string());
    }
    def
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    documents: HashMap<String, Vec<Symbol>>,
    top_level_by_name: HashMap<String, Definition>,
    enum_variant_by_name: HashMap<String, Definition>,
    superclass_by_name: HashMap<String, String>,
    member_by_type: HashMap<String, HashMap<String, Definition>>,
    annotated_members_by_type: HashMap<String, HashMap<String, Vec<Definition>>>,
    doc_idents: HashMap<String, HashMap<String, Vec<std::ops::Range<usize>>>>,
    generation: u64,
}

impl WorkspaceIndex {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn update_document(&mut self, uri: impl Into<String>, document: &ParsedDocument) {
        let uri: String = uri.into();
        self.remove_from_indices(&uri);
        self.doc_idents.remove(&uri);
        let all_symbols = document.symbols.all().to_vec();
        self.insert_into_indices(&uri, &all_symbols);
        self.doc_idents
            .insert(uri.clone(), scan_ident_occurrences(document));
        self.documents.insert(uri, all_symbols);
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn remove_document(&mut self, uri: &str) {
        self.remove_from_indices(uri);
        self.doc_idents.remove(uri);
        self.documents.remove(uri);
        self.generation = self.generation.wrapping_add(1);
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
                if sym.kind == SymbolKind::Function {
                    if let Some(target) = annotation_target_class(&sym) {
                        if let Some(by_name) = self.annotated_members_by_type.get_mut(target) {
                            if let Some(defs) = by_name.get_mut(&sym.name) {
                                defs.retain(|d| d.uri != uri);
                                if defs.is_empty() {
                                    by_name.remove(&sym.name);
                                }
                            }
                            if by_name.is_empty() {
                                self.annotated_members_by_type.remove(target);
                            }
                        }
                    }
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
                if sym.kind == SymbolKind::EnumVariant
                    && self
                        .enum_variant_by_name
                        .get(&sym.name)
                        .map(|d| d.uri == uri)
                        .unwrap_or(false)
                {
                    self.enum_variant_by_name.remove(&sym.name);
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
                if sym.kind == SymbolKind::Function {
                    if let Some(target) = annotation_target_class(sym) {
                        self.annotated_members_by_type
                            .entry(target.to_string())
                            .or_default()
                            .entry(sym.name.clone())
                            .or_default()
                            .push(Definition {
                                uri: uri.to_string(),
                                symbol: sym.clone(),
                            });
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
                if sym.kind == SymbolKind::EnumVariant {
                    self.enum_variant_by_name.insert(
                        sym.name.clone(),
                        Definition {
                            uri: uri.to_string(),
                            symbol: sym.clone(),
                        },
                    );
                }
            }
        }
    }

    pub fn find_top_level(&self, name: &str) -> Option<Definition> {
        self.top_level_by_name.get(name).cloned()
    }

    pub fn find_enum_variant(&self, name: &str) -> Option<Definition> {
        self.enum_variant_by_name.get(name).cloned()
    }

    pub fn all_enum_variants(&self) -> Vec<Definition> {
        self.enum_variant_by_name.values().cloned().collect()
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

    /// Unlike `top_level_by_name`, does not dedup by name — name collisions stay visible.
    pub fn all_top_level(&self) -> impl Iterator<Item = (&str, &Symbol)> {
        self.documents.iter().flat_map(|(uri, symbols)| {
            symbols
                .iter()
                .filter(|sym| sym.container.is_none())
                .map(move |sym| (uri.as_str(), sym))
        })
    }

    pub fn documents(&self) -> impl Iterator<Item = (&str, &[Symbol])> {
        self.documents
            .iter()
            .map(|(uri, syms)| (uri.as_str(), syms.as_slice()))
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
            .or_else(|| {
                self.annotated_members_by_type
                    .get(container_name)
                    .and_then(|members| members.get(name))
                    .and_then(|defs| defs.first())
            })
            .filter(|def| def.symbol.access >= min_access)
            .cloned()
    }

    pub fn direct_members_of(
        &self,
        container_name: &str,
        min_access: AccessLevel,
    ) -> Vec<Definition> {
        let class_body = self
            .member_by_type
            .get(container_name)
            .into_iter()
            .flat_map(|m| m.values().cloned());
        let annotated = self
            .annotated_members_by_type
            .get(container_name)
            .into_iter()
            .flat_map(|m| m.values().flatten().cloned());
        class_body
            .chain(annotated)
            .filter(|d| d.symbol.access >= min_access)
            .collect()
    }

    fn annotated_members(&self, container_name: &str, name: &str) -> Vec<Definition> {
        self.annotated_members_by_type
            .get(container_name)
            .and_then(|m| m.get(name))
            .cloned()
            .unwrap_or_default()
    }

    pub fn superclass_of(&self, class_name: &str) -> Option<String> {
        self.superclass_by_name.get(class_name).cloned()
    }

    pub fn parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<String> {
        self.full_parameters_of(uri, callable_id)
            .into_iter()
            .filter(|s| !s.is_optional)
            .map(|s| s.name)
            .collect()
    }

    pub fn full_parameters_of(&self, uri: &str, callable_id: SymbolId) -> Vec<Symbol> {
        let Some(symbols) = self.documents.get(uri) else {
            return vec![];
        };
        symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Parameter && s.container == Some(callable_id))
            .cloned()
            .collect()
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
    resolve_definition_at_byte(uri, document, db, byte_offset)
}

pub fn resolve_definition_at_byte(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<Definition> {
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
        .or_else(|| db.find_enum_variant(name))
        .or_else(|| db.find_script_global(name))
        .or_else(|| resolve_at_definition_site(uri, document, byte_offset, name))
}

/// All declaration sites at `position`, class-body declaration first.
pub fn resolve_all_definitions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    let Some(primary) = resolve_definition(uri, document, db, position) else {
        return Vec::new();
    };

    let Some((container, name)) = logical_member(&primary.symbol) else {
        return vec![primary];
    };

    let mut decls = db.all_member_declarations(&container, &name);
    if !decls
        .iter()
        .any(|d| definition_key(d) == definition_key(&primary))
    {
        decls.push(primary);
    }
    dedup_definitions(decls)
}

#[derive(Debug, Clone)]
pub struct SignatureHelpInfo {
    pub label: String,
    /// `[start, end)` UTF-16 offsets of each parameter substring within `label`.
    pub parameters: Vec<(u32, u32)>,
    pub active_parameter: Option<u32>,
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

    let callee_ident = match call.callee.kind() {
        "ident" => call.callee,
        "member_access_expr" | "incomplete_member_access_expr" => call
            .callee
            .child_by_field_name("member")
            .filter(|m| m.kind() == "ident")?,
        _ => return None,
    };
    let definition = resolve_definition_at_byte(uri, document, db, callee_ident.start_byte())?;
    if !matches!(
        definition.symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Event
    ) {
        return None;
    }

    let params = db.full_parameters_of(&definition.uri, definition.symbol.id);
    let colon = if compact_colon { ": " } else { " : " };

    let mut label = String::new();
    label.push_str(&definition.symbol.name);
    label.push('(');
    let mut parameters = Vec::with_capacity(params.len());
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let start = label.encode_utf16().count() as u32;
        if param.is_optional {
            label.push_str("optional ");
        }
        if param.is_out {
            label.push_str("out ");
        }
        label.push_str(&param.name);
        if let Some(ty) = &param.type_annotation {
            label.push_str(colon);
            label.push_str(ty);
        }
        let end = label.encode_utf16().count() as u32;
        parameters.push((start, end));
    }
    label.push(')');
    if let Some(ret) = &definition.symbol.type_annotation {
        if ret != "void" {
            label.push_str(colon);
            label.push_str(ret);
        }
    }

    let active_parameter = if params.is_empty() {
        None
    } else {
        let comma_count = call
            .args
            .map(|args| {
                let mut cursor = args.walk();
                args.children(&mut cursor)
                    .filter(|c| c.kind() == "," && c.start_byte() < byte_offset)
                    .count()
            })
            .unwrap_or(0);
        Some((comma_count as u32).min(params.len() as u32 - 1))
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
        "func_call_expr" => {
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
                callee: node.child_by_field_name("func")?,
                open_paren_byte: open.start_byte(),
                args: node.child_by_field_name("args"),
            })
        }
        "ERROR" => {
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
                .filter(|c| c.kind() == "func_call_args")
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
            let params_and_return = symbol.signature.as_deref().unwrap_or("");
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
            if let Some(sig) = &symbol.signature {
                let flavour_prefix = symbol
                    .flavour
                    .as_deref()
                    .map(|f| format!("{f} "))
                    .unwrap_or_default();
                lines.push(format!("{flavour_prefix}{label} {}{sig}", symbol.name));
            } else if let Some(type_annotation) = &symbol.type_annotation {
                lines.push(format!("{label} {} : {type_annotation}", symbol.name));
            } else {
                lines.push(format!("{label} {}", symbol.name));
            }
            if let Some(detail) = &symbol.detail {
                match lines.last_mut() {
                    Some(last) => {
                        last.push(' ');
                        last.push_str(detail);
                    }
                    None => lines.push(detail.clone()),
                }
            }
        }
    }

    lines.join("\n")
}

pub fn infer_expr_type_memo(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    node: Node,
    context_byte: usize,
    memo: &mut HashMap<(usize, usize), Option<String>>,
) -> Option<String> {
    let key = (node.start_byte(), node.end_byte());
    if let Some(cached) = memo.get(&key) {
        return cached.clone();
    }
    let value = infer_expr_type(uri, document, db, node, context_byte);
    memo.insert(key, value.clone());
    value
}

pub(crate) fn infer_expr_type(
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
                    let current_type = current_type_name(document, db, context_byte)?;
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
                .or_else(|| db.find_enum_variant(name))
                .and_then(|def| {
                    def.symbol.type_annotation.clone().or_else(|| {
                        if def.symbol.kind == SymbolKind::EnumVariant {
                            def.symbol.container_name.clone()
                        } else {
                            None
                        }
                    })
                })
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
        "this_expr" => current_type_name(document, db, context_byte),
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
            let current_type = current_type_name(document, db, ident.start_byte())?;
            resolve_document_member(uri, document, &current_type, name, AccessLevel::Private)
                .or_else(|| db.find_member(&current_type, name, AccessLevel::Private))
        }
        "super_expr" | "virtual_parent_expr" => {
            let current_type = enclosing_type_context(document, db, ident.start_byte())?;
            db.find_member(
                current_type.base_class.as_deref()?,
                name,
                AccessLevel::Protected,
            )
        }
        "parent_expr" => {
            let current_type = enclosing_type_context(document, db, ident.start_byte())?;
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
                        let current_type = current_type_name(document, db, ident.start_byte())?;
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
    let current_type = current_type_name(document, db, byte_offset)?;
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

fn current_type_name(
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<String> {
    enclosing_type_context(document, db, byte_offset).map(|ctx| ctx.name)
}

fn current_type_symbol(document: &ParsedDocument, byte_offset: usize) -> Option<&Symbol> {
    document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
    )
}

/// Falls back to the annotation target when not syntactically inside a type.
fn enclosing_type_context(
    document: &ParsedDocument,
    db: &SymbolDb,
    byte_offset: usize,
) -> Option<TypeContext> {
    if let Some(symbol) = current_type_symbol(document, byte_offset) {
        return Some(TypeContext {
            name: symbol.name.clone(),
            base_class: symbol.base_class.clone(),
            owner_class: symbol.owner_class.clone(),
        });
    }

    let callable = document.symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    )?;
    if callable.container.is_some() || callable.kind != SymbolKind::Function {
        return None;
    }
    let target = annotation_target_class(callable)?;
    let class = db.find_top_level(target);
    Some(TypeContext {
        name: target.to_string(),
        base_class: class.as_ref().and_then(|def| def.symbol.base_class.clone()),
        owner_class: class.and_then(|def| def.symbol.owner_class.clone()),
    })
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

/// The `(container, member name)` a symbol logically belongs to, resolving
/// annotation functions to the class they target.
fn logical_member(symbol: &Symbol) -> Option<(String, String)> {
    match symbol.kind {
        SymbolKind::Method | SymbolKind::Field => symbol
            .container_name
            .as_deref()
            .map(|cn| (cn.to_string(), symbol.name.clone())),
        SymbolKind::Function if symbol.container.is_none() => {
            annotation_target_class(symbol).map(|t| (t.to_string(), symbol.name.clone()))
        }
        _ => None,
    }
}

fn definition_key(definition: &Definition) -> (String, std::ops::Range<usize>) {
    (
        definition.uri.clone(),
        definition.symbol.selection_byte_range.clone(),
    )
}

/// Every declaration of the same logical member as `definition`, including
/// `definition` itself.
fn member_equivalence_set(definition: &Definition, db: &SymbolDb) -> Vec<Definition> {
    let Some((container, name)) = logical_member(&definition.symbol) else {
        return vec![definition.clone()];
    };
    let mut decls = db.all_member_declarations(&container, &name);
    if !decls
        .iter()
        .any(|d| definition_key(d) == definition_key(definition))
    {
        decls.push(definition.clone());
    }
    dedup_definitions(decls)
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

    // All declarations of the logical member count as one symbol.
    let equiv = member_equivalence_set(definition, db);
    let equiv_keys: Vec<(String, std::ops::Range<usize>)> =
        equiv.iter().map(definition_key).collect();

    let scope = if equiv.len() > 1 {
        SearchScope::AllDocuments
    } else {
        definition_search_scope(definition, definition_document)
    };

    let mut results = Vec::new();
    let mut decl_found = vec![false; equiv.len()];

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
                Some(ref r) if equiv_keys.contains(&definition_key(r)) => {}
                _ => continue,
            }

            let occurrence_key = (uri.to_string(), byte_range.clone());
            if let Some(idx) = equiv_keys.iter().position(|k| *k == occurrence_key) {
                decl_found[idx] = true;
                if !include_declaration {
                    continue;
                }
            }
            let range = document.line_index.byte_range_to_range(
                &document.source,
                byte_range.start,
                byte_range.end,
            );
            results.push((uri.to_string(), range));
        }
    }

    // Catch declarations whose file was not in the search set.
    if include_declaration {
        for (idx, decl) in equiv.iter().enumerate() {
            if !decl_found[idx] {
                results.push((decl.uri.clone(), decl.symbol.selection_range));
            }
        }
    }

    results
}

fn dedup_by_name(defs: impl Iterator<Item = Definition>) -> Vec<Definition> {
    let mut seen: HashMap<String, Definition> = HashMap::new();
    for def in defs {
        seen.entry(def.symbol.name.clone()).or_insert(def);
    }
    seen.into_values().collect()
}

/// `Definition` has no `Eq`; identity is `(uri, selection byte range)`.
fn dedup_definitions(defs: Vec<Definition>) -> Vec<Definition> {
    let mut seen: Vec<(String, std::ops::Range<usize>)> = Vec::new();
    let mut result = Vec::new();
    for def in defs {
        let key = (def.uri.clone(), def.symbol.selection_byte_range.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        result.push(def);
    }
    result
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

    let current_type = enclosing_type_context(document, db, byte_offset)?;
    match node.kind() {
        "this_expr" => resolve_document_top_level(uri, document, &current_type.name)
            .or_else(|| db.find_top_level(&current_type.name)),
        "super_expr" => {
            let base_name = current_type.base_class.as_deref()?;
            resolve_document_top_level(uri, document, base_name)
                .or_else(|| db.find_top_level(base_name))
        }
        "parent_expr" => {
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
            let current_type = enclosing_type_context(document, db, context_byte)?;
            current_type.base_class?
        }
        "parent_expr" | "parent" => {
            let current_type = enclosing_type_context(document, db, context_byte)?;
            current_type.owner_class?
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

fn nearest_enclosing_block<'a>(mut node: Node<'a>) -> Option<Node<'a>> {
    const BLOCKS: &[&str] = &["func_block", "switch_block", "member_default_val_block"];
    loop {
        if BLOCKS.contains(&node.kind()) {
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

/// Nearest node before `byte_offset`, skipping whitespace and comments.
fn significant_node_before_byte<'a>(
    root: Node<'a>,
    source: &[u8],
    byte_offset: usize,
) -> Option<Node<'a>> {
    let mut end = byte_offset;
    loop {
        let p = source[..end]
            .iter()
            .rposition(|&b| !b.is_ascii_whitespace())?;
        let node = root.descendant_for_byte_range(p, p + 1)?;
        if node.kind() != "comment" {
            return Some(node);
        }
        end = node.start_byte();
    }
}

fn is_statement_boundary(node: Node) -> bool {
    if node.has_error() {
        return false;
    }
    if matches!(node.kind(), "{" | "}" | ";") {
        return true;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    // `)` closing an if condition without a curly-brace body is a statement boundary.
    let is_single_line_if = node.kind() == ")" && parent.kind() == "if_stmt";
    if is_single_line_if {
        return true;
    }
    if parent.kind() == "else_stmt" {
        return true;
    }
    // `:` closing a switch case/default label is also a statement boundary.
    node.kind() == ":" && matches!(parent.kind(), "switch_case_label" | "switch_default_label")
}

fn is_type_annotation_boundary(node: Node) -> bool {
    if node.has_error() {
        return false;
    }
    node.kind() == ":"
        && !node.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                "switch_case_label" | "switch_default_label" | "ternary_cond_expr"
            )
        })
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
    let nodes = nodes_at_offset(root, byte_offset);
    let source = document.source.as_bytes();

    let in_type_context =
        // Gate 1: cursor immediately after a type-annotation colon
        significant_node_before_byte(root, source, byte_offset).is_some_and(is_type_annotation_boundary)
        // Gate 2: cursor on/within an ident whose start follows a type-annotation colon
        || nodes
            .last()
            .filter(|&n| is_kind_or_error_wrapped_kind(*n, &["ident"]))
            .and_then(|n| significant_node_before_byte(root, source, n.start_byte()))
            .is_some_and(is_type_annotation_boundary)
        // Gate 3: cursor already inside a type_annot subtree (generic type args, clean parses)
        || nodes.iter().any(|n| has_type_annot_ancestor(*n));

    if !in_type_context {
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

pub fn annotation_name_completions(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<SourcePosition> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);

    let node = nodes.iter().find(|n| n.kind() == "annotation_ident")?;
    let prev = significant_node_before_byte(root, document.source.as_bytes(), node.start_byte());
    if prev.is_some_and(|p| !is_statement_boundary(p)) {
        return None;
    }
    Some(
        document
            .line_index
            .byte_to_position(&document.source, node.start_byte()),
    )
}

pub fn annotation_arg_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    annotation_arg_completions_inner(document, db, position).unwrap_or_default()
}

fn annotation_arg_completions_inner(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<Vec<Definition>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let in_annotation_arg = nodes_at_offset(root, byte_offset)
        .into_iter()
        .any(|n| has_annotation_arg_ancestor(n, byte_offset, &document.source));

    if !in_annotation_arg {
        return None;
    }

    Some(
        db.all_types()
            .into_iter()
            .filter(|def| def.symbol.kind == SymbolKind::Class)
            .collect(),
    )
}

const CLASS_ARG_ANNOTATIONS: &[&str] =
    &["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"];

fn has_annotation_arg_ancestor(node: Node, byte_offset: usize, source: &str) -> bool {
    let mut current = node;
    loop {
        if current.kind() == "annotation" {
            return takes_class_arg(current, source)
                && is_inside_annotation_parens(current, byte_offset);
        }
        match current.parent() {
            Some(p) => current = p,
            None => return false,
        }
    }
}

fn takes_class_arg(annotation: Node, source: &str) -> bool {
    annotation
        .children(&mut annotation.walk())
        .find(|c| c.kind() == "annotation_ident")
        .map(|n| &source[n.start_byte()..n.end_byte()])
        .is_some_and(|name| CLASS_ARG_ANNOTATIONS.contains(&name))
}

fn is_inside_annotation_parens(annotation: Node, byte_offset: usize) -> bool {
    let mut cursor = annotation.walk();
    let mut saw_open = false;
    for child in annotation.children(&mut cursor) {
        match child.kind() {
            "(" => saw_open = true,
            ")" => {
                if byte_offset <= child.start_byte() {
                    return saw_open;
                }
                return false;
            }
            _ => {}
        }
    }
    saw_open
}

#[derive(Debug)]
pub enum AfterWrapMethodCompletions {
    /// Cursor is directly after `@wrapMethod(CClass)` — only `function` is valid next.
    FunctionKeyword,
    /// Cursor is after `@wrapMethod(CClass)\nfunction ` — offer methods of the class.
    MethodList(Vec<Definition>),
}

pub fn after_wrap_method_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<AfterWrapMethodCompletions> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let source = document.source.as_bytes();

    // If the cursor is ON an ident or `function` token, step back to the node
    // before that token's start; otherwise step back from the cursor directly.
    let effective_prev = nodes_at_offset(root, byte_offset)
        .last()
        .filter(|n| matches!(n.kind(), "ident" | "function"))
        .and_then(|n| significant_node_before_byte(root, source, n.start_byte()))
        .or_else(|| significant_node_before_byte(root, source, byte_offset))?;

    // Stage 2: `function` keyword is the boundary — cursor is after it or typing a name.
    if effective_prev.kind() == "function" {
        let before_fn = significant_node_before_byte(root, source, effective_prev.start_byte())?;
        let class_name = wrap_method_class_from_closing_paren(before_fn, &document.source)?;
        return Some(AfterWrapMethodCompletions::MethodList(
            direct_methods_of_class(class_name, db)?,
        ));
    }

    // Stage 1: `)` of annotation is the boundary — `function` keyword not yet complete.
    let class_name = wrap_method_class_from_closing_paren(effective_prev, &document.source)?;
    let class_def = db.find_top_level(class_name)?;
    if class_def.symbol.kind != SymbolKind::Class {
        return None;
    }
    Some(AfterWrapMethodCompletions::FunctionKeyword)
}

fn direct_methods_of_class(class_name: &str, db: &SymbolDb) -> Option<Vec<Definition>> {
    let class_def = db.find_top_level(class_name)?;
    if class_def.symbol.kind != SymbolKind::Class {
        return None;
    }
    Some(
        db.direct_members_of(class_name, AccessLevel::Private)
            .into_iter()
            .filter(|def| matches!(def.symbol.kind, SymbolKind::Method | SymbolKind::Event))
            .collect(),
    )
}

fn wrap_method_class_from_closing_paren<'a>(node: Node, source: &'a str) -> Option<&'a str> {
    if node.kind() != ")" {
        return None;
    }
    let annotation = node.parent()?;
    if annotation.kind() != "annotation" {
        return None;
    }
    let annotation_name = annotation
        .children(&mut annotation.walk())
        .find(|c| c.kind() == "annotation_ident")
        .map(|n| &source[n.start_byte()..n.end_byte()])?;
    if !matches!(annotation_name, "@wrapMethod" | "@replaceMethod") {
        return None;
    }
    annotation
        .children(&mut annotation.walk())
        .find(|c| c.kind() == "ident")
        .map(|n| &source[n.start_byte()..n.end_byte()])
}

pub fn extends_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    let Some(header) = header_state_and_kind(document, position) else {
        return Vec::new();
    };
    if header.state != HeaderState::AfterExtendsKw {
        return Vec::new();
    }
    let self_name = header.self_name.as_deref();
    match header.kind {
        Some(HeaderDeclKind::Class) => db
            .all_types()
            .into_iter()
            .filter(|def| def.symbol.kind == SymbolKind::Class)
            .filter(|def| Some(def.symbol.name.as_str()) != self_name)
            .collect(),
        Some(HeaderDeclKind::State) => {
            let Some(owner) = header.owner_name.as_deref() else {
                return Vec::new();
            };
            let chain = class_chain(db, owner);
            if chain.is_empty() {
                return Vec::new();
            }
            db.all_types()
                .into_iter()
                .filter(|def| def.symbol.kind == SymbolKind::State)
                .filter(|def| {
                    def.symbol
                        .owner_class
                        .as_deref()
                        .is_some_and(|o| chain.iter().any(|c| c == o))
                })
                .filter(|def| Some(def.symbol.name.as_str()) != self_name)
                .collect()
        }
        None => Vec::new(),
    }
}

pub fn state_owner_completions(
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Vec<Definition> {
    let Some(header) = header_state_and_kind(document, position) else {
        return Vec::new();
    };
    if header.state != HeaderState::AfterInKw {
        return Vec::new();
    }
    db.all_types()
        .into_iter()
        .filter(|def| def.symbol.kind == SymbolKind::Class)
        .collect()
}

pub fn class_header_keyword_completions(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Vec<&'static str> {
    let Some(header) = header_state_and_kind(document, position) else {
        return Vec::new();
    };
    match header.state {
        HeaderState::AfterClassName | HeaderState::AfterOwner => vec!["extends"],
        HeaderState::AfterStateName => vec!["in"],
        _ => Vec::new(),
    }
}

fn class_chain(db: &SymbolDb, start: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut current = start.to_string();
    for _ in 0..=MAX_INHERITANCE_DEPTH {
        if chain.iter().any(|c| c == &current) {
            break;
        }
        chain.push(current.clone());
        match db.superclass_of(&current) {
            Some(next) => current = next,
            None => break,
        }
    }
    chain
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HeaderState {
    Initial,
    AfterClassKw,
    AfterClassName,
    AfterStateKw,
    AfterStateName,
    AfterInKw,
    AfterOwner,
    AfterExtendsKw,
    AfterBase,
    Body,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HeaderDeclKind {
    Class,
    State,
}

struct HeaderContext {
    state: HeaderState,
    kind: Option<HeaderDeclKind>,
    self_name: Option<String>,
    owner_name: Option<String>,
}

fn header_state_and_kind(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<HeaderContext> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;
    let root = document.tree.root_node();

    let direct: Vec<Node> = nodes_at_offset(root, byte_offset)
        .into_iter()
        .filter(|n| n.kind() != "script")
        .collect();

    let header_node = direct
        .iter()
        .find_map(|n| enclosing_header_node(*n))
        .or_else(|| {
            let mut tc = root.walk();
            root.children(&mut tc)
                .take_while(|c| c.end_byte() <= byte_offset)
                .last()
                .and_then(enclosing_header_node)
        })?;

    let mut ctx = HeaderContext {
        state: HeaderState::Initial,
        kind: None,
        self_name: None,
        owner_name: None,
    };
    header_walk(
        header_node,
        byte_offset,
        document.source.as_bytes(),
        &mut ctx,
    );
    Some(ctx)
}

fn enclosing_header_node(start: Node) -> Option<Node> {
    let mut current = start;
    loop {
        match current.kind() {
            "class_decl" | "state_decl" => return Some(current),
            "ERROR" => {
                if let Some(p) = current.parent() {
                    if matches!(p.kind(), "class_decl" | "state_decl") {
                        return Some(p);
                    }
                }
                if node_contains_kind_any(current, &["class", "state"]) {
                    return Some(current);
                }
            }
            _ => {}
        }
        current = current.parent()?;
    }
}

fn header_walk(node: Node, byte_offset: usize, source: &[u8], ctx: &mut HeaderContext) {
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        if child.start_byte() >= byte_offset {
            break;
        }
        let past = child.end_byte() < byte_offset;
        match (ctx.state, child.kind()) {
            (HeaderState::Initial, "class") => {
                ctx.state = HeaderState::AfterClassKw;
                ctx.kind = Some(HeaderDeclKind::Class);
            }
            (HeaderState::Initial, "state") => {
                ctx.state = HeaderState::AfterStateKw;
                ctx.kind = Some(HeaderDeclKind::State);
            }
            (HeaderState::AfterClassKw, "ident") if past => {
                ctx.state = HeaderState::AfterClassName;
                ctx.self_name = child.utf8_text(source).ok().map(str::to_string);
            }
            (HeaderState::AfterStateKw, "ident") if past => {
                ctx.state = HeaderState::AfterStateName;
                ctx.self_name = child.utf8_text(source).ok().map(str::to_string);
            }
            (HeaderState::AfterStateName, "in") => ctx.state = HeaderState::AfterInKw,
            (HeaderState::AfterInKw, "ident") if past => {
                ctx.state = HeaderState::AfterOwner;
                ctx.owner_name = child.utf8_text(source).ok().map(str::to_string);
            }
            (HeaderState::AfterClassName | HeaderState::AfterOwner, "extends") => {
                ctx.state = HeaderState::AfterExtendsKw;
            }
            (HeaderState::AfterExtendsKw, "ident") if past => ctx.state = HeaderState::AfterBase,
            (_, "class_def" | "{") => ctx.state = HeaderState::Body,
            (_, "ERROR") => header_walk(child, byte_offset, source, ctx),
            _ => {}
        }
    }
}

fn node_contains_kind_any(node: Node, kinds: &[&str]) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|c| kinds.contains(&c.kind()));
    found
}

fn is_kind_or_error_wrapped_kind(node: Node, kinds: &[&str]) -> bool {
    let effective = if node.is_error() && node.child_count() == 1 {
        node.child(0).unwrap()
    } else {
        node
    };
    kinds.contains(&effective.kind())
}

pub struct StatementCompletions {
    pub locals: Vec<Definition>,
    pub members: Vec<Definition>,
    pub globals: Vec<Definition>,
    pub has_this: bool,
    pub has_super: bool,
    pub in_switch: bool,
    pub in_loop: bool,
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
        in_switch: false,
        in_loop: false,
    })
}

fn statement_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<StatementCompletions> {
    const STMT_WRITER_KINDS: &[&str] = &[
        "ident", "var", "this", "super", "if", "else", "do", "while", "for", "switch", "return",
        "case", "default",
    ];
    let (nodes, base) = function_body_completions(
        uri,
        document,
        db,
        position,
        is_statement_boundary,
        STMT_WRITER_KINDS,
    )?;

    let in_switch = nodes
        .iter()
        .any(|n| nearest_enclosing_block(*n).is_some_and(|b| b.kind() == "switch_block"));

    let in_loop = nodes
        .iter()
        .any(|n| find_ancestor_of_kind(*n, &["for_stmt", "while_stmt", "do_while_stmt"]).is_some());

    Some(StatementCompletions {
        locals: base.locals,
        members: base.members,
        globals: base.globals,
        has_this: base.has_this,
        has_super: base.has_super,
        in_switch,
        in_loop,
    })
}

pub struct ExpressionCompletions {
    pub locals: Vec<Definition>,
    pub members: Vec<Definition>,
    pub globals: Vec<Definition>,
    pub has_this: bool,
    pub has_super: bool,
}

pub fn expression_completions(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<ExpressionCompletions> {
    expression_completions_inner(uri, document, db, position)
}

fn is_expression_boundary(node: Node) -> bool {
    matches!(
        node.kind(),
        "(" | ","
            | "="
            | "return"
            | "assign_op_direct"
            | "assign_op_sum"
            | "assign_op_diff"
            | "assign_op_mult"
            | "assign_op_div"
            | "assign_op_bitand"
            | "assign_op_bitor"
            | "binary_op_or"
            | "binary_op_and"
            | "binary_op_bitor"
            | "binary_op_bitand"
            | "binary_op_bitxor"
            | "binary_op_eq"
            | "binary_op_neq"
            | "binary_op_gt"
            | "binary_op_ge"
            | "binary_op_lt"
            | "binary_op_le"
            | "binary_op_diff"
            | "binary_op_sum"
            | "binary_op_mod"
            | "binary_op_div"
            | "binary_op_mult"
            | "unary_op_neg"
            | "unary_op_not"
            | "unary_op_bitnot"
            | "unary_op_plus"
    )
}

fn expression_completions_inner(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
) -> Option<ExpressionCompletions> {
    let (_, base) = function_body_completions(
        uri,
        document,
        db,
        position,
        is_expression_boundary,
        &["ident"],
    )?;

    Some(ExpressionCompletions {
        locals: base.locals,
        members: base.members,
        globals: base.globals,
        has_this: base.has_this,
        has_super: base.has_super,
    })
}

struct FunctionBodyContext {
    locals: Vec<Definition>,
    members: Vec<Definition>,
    globals: Vec<Definition>,
    has_this: bool,
    has_super: bool,
}

fn function_body_completions<'a>(
    uri: &str,
    document: &'a ParsedDocument,
    db: &SymbolDb,
    position: SourcePosition,
    boundary: fn(Node) -> bool,
    writer_kinds: &[&str],
) -> Option<(Vec<Node<'a>>, FunctionBodyContext)> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);

    let prev = significant_node_before_byte(root, document.source.as_bytes(), byte_offset);
    let at_start = prev.is_some_and(boundary);
    let writing_first = nodes
        .last()
        .filter(|&n| is_kind_or_error_wrapped_kind(*n, writer_kinds))
        .and_then(|n| {
            significant_node_before_byte(root, document.source.as_bytes(), n.start_byte())
        })
        .is_some_and(boundary);
    if !at_start && !writing_first {
        return None;
    }

    if !nodes
        .iter()
        .any(|n| find_ancestor_of_kind(*n, &["func_block"]).is_some())
    {
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

    let current_type = enclosing_type_context(document, db, byte_offset);
    let members: Vec<Definition> = current_type
        .as_ref()
        .map(|t| db.members_of(&t.name, AccessLevel::Private))
        .unwrap_or_default();
    let has_this = current_type.is_some();
    let has_super = current_type
        .as_ref()
        .and_then(|t| t.base_class.as_deref())
        .is_some();

    let mut globals = db.all_top_level_callables();
    globals.extend(db.all_script_globals());
    globals.extend(db.all_enum_variants());

    Some((
        nodes,
        FunctionBodyContext {
            locals,
            members,
            globals,
            has_this,
            has_super,
        },
    ))
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
    let class_body = nodes.iter().find_map(|n| enclosing_class_body_node(*n))?;

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

    if let Some(child) = class_body_child_at_cursor(class_body, byte_offset) {
        let cursor_inside = byte_offset < child.end_byte();
        if cursor_inside || child.is_error() {
            let limit = if cursor_inside {
                byte_offset
            } else {
                child.end_byte()
            };
            let mut cur = child.walk();
            for ch in child.children(&mut cur) {
                if ch.start_byte() >= limit {
                    break;
                }
                if ch.kind() == "specifier" {
                    match ch.utf8_text(document.source.as_bytes()).unwrap_or("") {
                        "private" | "protected" | "public" => ctx.has_access = true,
                        "import" => ctx.has_import = true,
                        "final" => ctx.has_final = true,
                        "latent" => ctx.has_latent = true,
                        "editable" => ctx.has_editable = true,
                        "saved" => ctx.has_saved = true,
                        "const" => ctx.has_const_ = true,
                        "inlined" => ctx.has_inlined = true,
                        "optional" => ctx.has_optional = true,
                        _ => {}
                    }
                } else if matches!(
                    ch.kind(),
                    "var" | "function" | "event" | "autobind" | "default" | "defaults" | "hint"
                ) {
                    ctx.saw_decl_keyword = true;
                    break;
                }
                // unknown token (partial ident etc.) — ignore, don't affect context
            }
        }
        // cursor after a complete declaration: ctx stays empty, offer all keywords
    }

    if ctx.saw_decl_keyword {
        return None;
    }

    Some(class_body_kw_candidates(&ctx))
}

fn enclosing_class_body_node(mut node: Node) -> Option<Node> {
    loop {
        match node.kind() {
            "func_block" | "member_default_val_block" | "script" => return None,
            "class_def" | "struct_def" => return Some(node),
            _ => node = node.parent()?,
        }
    }
}

fn class_body_child_at_cursor(class_body: Node, byte_offset: usize) -> Option<Node> {
    let mut cur = class_body.walk();
    let mut result = None;
    for child in class_body.children(&mut cur) {
        if !child.is_named() {
            continue;
        }
        if child.start_byte() > byte_offset {
            break;
        }
        result = Some(child);
    }
    result
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

#[derive(Clone, Copy)]
enum MemberAnnotation {
    Field,
    Method,
}

#[derive(Default)]
struct ScriptBodyCtx {
    has_import: bool,
    has_statemachine: bool,
    has_abstract: bool,
    has_final: bool,
    has_latent: bool,
    has_flavour: bool,
    member_annotation: Option<MemberAnnotation>,
    saw_decl_keyword: bool,
}

impl ScriptBodyCtx {
    fn has_any(&self) -> bool {
        self.has_import
            || self.has_statemachine
            || self.has_abstract
            || self.has_final
            || self.has_latent
            || self.has_flavour
    }
}

pub fn script_body_completions(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Vec<&'static str> {
    script_body_inner(document, position).unwrap_or_default()
}

fn script_body_inner(
    document: &ParsedDocument,
    position: SourcePosition,
) -> Option<Vec<&'static str>> {
    let byte_offset = document
        .line_index
        .position_to_byte(&document.source, position)?;

    let root = document.tree.root_node();
    let nodes = nodes_at_offset(root, byte_offset);

    nodes.iter().find_map(|n| enclosing_script_scope(*n))?;

    let mut ctx = ScriptBodyCtx::default();

    if let Some(child) = script_child_at_cursor(root, byte_offset) {
        let cursor_inside = byte_offset < child.end_byte();
        if cursor_inside || child.is_error() {
            let limit = if cursor_inside {
                byte_offset
            } else {
                child.end_byte()
            };
            collect_script_ctx(child, document.source.as_bytes(), limit, &mut ctx);
        }
    }

    if ctx.saw_decl_keyword {
        return None;
    }

    Some(script_body_candidates(&ctx))
}

fn collect_script_ctx(node: Node, source: &[u8], limit: usize, ctx: &mut ScriptBodyCtx) {
    let mut cur = node.walk();
    for ch in node.children(&mut cur) {
        if ch.start_byte() >= limit {
            break;
        }
        match ch.kind() {
            "specifier" => match ch.utf8_text(source).unwrap_or("") {
                "import" => ctx.has_import = true,
                "statemachine" => ctx.has_statemachine = true,
                "abstract" => ctx.has_abstract = true,
                "final" => ctx.has_final = true,
                "latent" => ctx.has_latent = true,
                _ => {}
            },
            "func_flavour" => ctx.has_flavour = true,
            "cleanup" | "entry" | "exec" | "quest" | "reward" | "storyscene" | "timer" => {
                ctx.has_flavour = true;
            }
            // @addField/@addMethod inject a class member — member specifiers follow.
            "annotation" => {
                let name = ch
                    .children(&mut ch.walk())
                    .find(|c| c.kind() == "annotation_ident")
                    .and_then(|n| n.utf8_text(source).ok());
                match name {
                    Some("@addField") => ctx.member_annotation = Some(MemberAnnotation::Field),
                    Some("@addMethod") => ctx.member_annotation = Some(MemberAnnotation::Method),
                    _ => {}
                }
            }
            "class" | "state" | "struct" | "enum" | "function" | "var" => {
                ctx.saw_decl_keyword = true;
                return;
            }
            "ERROR" => collect_script_ctx(ch, source, limit, ctx),
            _ => {}
        }
    }
}

fn enclosing_script_scope(mut node: Node) -> Option<Node> {
    loop {
        match node.kind() {
            "func_block"
            | "class_def"
            | "struct_def"
            | "member_default_val_block"
            | "switch_block" => return None,
            "script" => return Some(node),
            _ => {}
        }
        node = node.parent()?;
    }
}

fn script_child_at_cursor(script: Node, byte_offset: usize) -> Option<Node> {
    let mut cur = script.walk();
    let mut result = None;
    for child in script.children(&mut cur) {
        if !child.is_named() {
            continue;
        }
        if child.start_byte() > byte_offset {
            break;
        }
        result = Some(child);
    }
    result
}

fn script_body_candidates(ctx: &ScriptBodyCtx) -> Vec<&'static str> {
    if let Some(member) = ctx.member_annotation {
        return member_annotation_candidates(member, ctx);
    }

    let mut kw: Vec<&'static str> = Vec::new();

    let in_func_path = ctx.has_final || ctx.has_latent || ctx.has_flavour;

    let can_class = !in_func_path;
    let can_state = can_class && !ctx.has_statemachine;
    let can_struct = can_state && !ctx.has_abstract;
    let can_enum = !ctx.has_any();
    let can_function = !ctx.has_statemachine && !ctx.has_abstract;
    let can_var = !ctx.has_any();

    if !ctx.has_import && !in_func_path {
        kw.push("import");
    }
    if !ctx.has_statemachine && !in_func_path && !ctx.has_abstract {
        kw.push("statemachine");
    }
    if !ctx.has_abstract && !in_func_path {
        kw.push("abstract");
    }
    if !ctx.has_final && can_function && !ctx.has_latent && !ctx.has_flavour {
        kw.push("final");
    }
    if !ctx.has_latent && can_function && !ctx.has_flavour {
        kw.push("latent");
    }
    if !ctx.has_flavour && can_function {
        kw.extend_from_slice(&[
            "cleanup",
            "entry",
            "exec",
            "quest",
            "reward",
            "storyscene",
            "timer",
        ]);
    }

    if can_class {
        kw.push("class");
    }
    if can_state {
        kw.push("state");
    }
    if can_struct {
        kw.push("struct");
    }
    if can_enum {
        kw.push("enum");
    }
    if can_function {
        kw.push("function");
    }
    if can_var {
        kw.push("var");
    }

    if !ctx.has_any() {
        kw.extend_from_slice(MODDING_ANNOTATIONS);
    }

    kw
}

fn member_annotation_candidates(
    member: MemberAnnotation,
    ctx: &ScriptBodyCtx,
) -> Vec<&'static str> {
    let mut kw: Vec<&'static str> = Vec::new();

    // Access modifiers, when present, must come before any other specifier.
    if !ctx.has_any() {
        kw.extend_from_slice(&["private", "protected", "public"]);
    }

    match member {
        MemberAnnotation::Field => {
            kw.extend_from_slice(&["editable", "saved", "const", "inlined", "var"]);
        }
        MemberAnnotation::Method => {
            if !ctx.has_final {
                kw.push("final");
            }
            if !ctx.has_latent {
                kw.push("latent");
            }
            kw.extend_from_slice(&["function", "event"]);
        }
    }

    kw
}

#[cfg(test)]
mod tests;
