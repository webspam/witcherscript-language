use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::kinds;
use crate::cst::walk::{CstVisitor, Visit, walk};
use crate::document::ParsedDocument;
use crate::resolve::{SymbolDb, classify_definition_at_ident};
use crate::symbols::{SymbolId, SymbolKind};

pub const TOKEN_TYPES: &[&str] = &[
    "class",      // 0
    "enum",       // 1
    "enumMember", // 2
    "function",   // 3
    "parameter",  // 4
    "variable",   // 5
    "property",   // 6
    "keyword",    // 7
    "comment",    // 8
    "string",     // 9
    "number",     // 10
    "type",       // 11
    "decorator",  // 12 (annotation names)
    "modifier",   // 13 (access/storage specifiers and declaration keywords)
                  // NOTE: "type" (index 11) is registered to preserve indices but is never emitted;
                  // type-annotation idents are resolved and classified by their actual symbol kind.
];

pub const TOKEN_MODIFIERS: &[&str] = &["declaration", "defaultLibrary"];

const TT_CLASS: u32 = 0;
const TT_ENUM: u32 = 1;
const TT_ENUM_MEMBER: u32 = 2;
const TT_FUNCTION: u32 = 3;
const TT_PARAMETER: u32 = 4;
const TT_VARIABLE: u32 = 5;
const TT_PROPERTY: u32 = 6;
const TT_COMMENT: u32 = 8;
const TT_STRING: u32 = 9;
const TT_NUMBER: u32 = 10;
// index 11 ("type") is registered in TOKEN_TYPES to preserve indices but never emitted.
const TT_DECORATOR: u32 = 12;
const TT_MODIFIER: u32 = 13;

const MOD_DEFAULT_LIBRARY: u32 = 1 << 1;

struct RawToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
    token_modifiers: u32,
}

pub fn collect_semantic_tokens(uri: &str, document: &ParsedDocument, db: &SymbolDb) -> Vec<u32> {
    collect_semantic_tokens_cancellable(uri, document, db, &|| true)
        .expect("should_continue=|| true cannot cancel")
}

// Cancellation is checked between top-level decls; one decl runs to completion before bailing.
pub fn collect_semantic_tokens_cancellable(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    should_continue: &dyn Fn() -> bool,
) -> Option<Vec<u32>> {
    let mut collector = TokenCollector {
        uri,
        document,
        db,
        cache: HashMap::new(),
        tokens: Vec::new(),
    };
    let root = document.tree.root_node();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if !should_continue() {
            return None;
        }
        walk(child, &mut collector);
    }
    Some(encode(&collector.tokens))
}

type ClassifyCache = HashMap<(String, Option<SymbolId>), Option<(u32, u32)>>;

struct TokenCollector<'a, 'db> {
    uri: &'a str,
    document: &'a ParsedDocument,
    db: &'a SymbolDb<'db>,
    cache: ClassifyCache,
    tokens: Vec<RawToken>,
}

impl<'tree> CstVisitor<'tree> for TokenCollector<'_, '_> {
    fn enter(&mut self, node: Node<'tree>) -> Visit {
        if let Some((token_type, token_modifiers)) =
            classify(node, self.uri, self.document, self.db, &mut self.cache)
        {
            let range = self.document.line_index.byte_range_to_range(
                &self.document.source,
                node.start_byte(),
                node.end_byte(),
            );
            if range.start.line == range.end.line && range.end.character > range.start.character {
                self.tokens.push(RawToken {
                    line: range.start.line,
                    start_char: range.start.character,
                    length: range.end.character - range.start.character,
                    token_type,
                    token_modifiers,
                });
            }
            Visit::SkipChildren
        } else if node.is_named() {
            Visit::Children
        } else {
            Visit::SkipChildren
        }
    }
}

fn classify(
    node: Node,
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    cache: &mut ClassifyCache,
) -> Option<(u32, u32)> {
    match node.kind() {
        kinds::IDENT => classify_ident(node, uri, document, db, cache),
        kinds::ANNOTATION_IDENT => Some((TT_DECORATOR, 0)),
        kinds::COMMENT => Some((TT_COMMENT, 0)),
        kinds::LITERAL_NAME => Some((TT_ENUM_MEMBER, 0)),
        kinds::LITERAL_STRING => Some((TT_STRING, 0)),
        kinds::LITERAL_INT | kinds::LITERAL_FLOAT | kinds::LITERAL_HEX => Some((TT_NUMBER, 0)),
        kinds::SPECIFIER | kinds::FUNC_FLAVOUR | kinds::AUTOBIND_SINGLE => Some((TT_MODIFIER, 0)),
        _ => {
            if node.is_named() {
                None
            } else {
                classify_anonymous_keyword(node.kind()).map(|t| (t, 0))
            }
        }
    }
}

fn classify_ident(
    node: Node,
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    cache: &mut ClassifyCache,
) -> Option<(u32, u32)> {
    let parent = node.parent()?;
    match parent.kind() {
        kinds::CLASS_DECL | kinds::STRUCT_DECL | kinds::STATE_DECL => Some((TT_CLASS, 0)),
        kinds::ENUM_DECL => Some((TT_ENUM, 0)),
        kinds::ENUM_MEMBER_DECL => Some((TT_ENUM_MEMBER, 0)),
        kinds::FUNC_DECL | kinds::EVENT_DECL => Some((TT_FUNCTION, 0)),
        kinds::FUNC_PARAM_GROUP => Some((TT_PARAMETER, 0)),
        kinds::MEMBER_VAR_DECL | kinds::AUTOBIND_DECL => Some((TT_PROPERTY, 0)),
        kinds::LOCAL_VAR_DECL_STMT => Some((TT_VARIABLE, 0)),
        _ => {
            if let Some(t) = classify_locally(node, document) {
                return Some((t, 0));
            }
            if is_member_access_rhs(node, parent) {
                return classify_definition_at_ident(uri, document, db, node)
                    .map(|def| (symbol_kind_to_token_type(def.symbol.kind), 0));
            }
            let name = node.utf8_text(document.source.as_bytes()).ok()?;
            let type_kinds = [SymbolKind::Class, SymbolKind::Struct, SymbolKind::State];
            let class_id = document
                .symbols
                .enclosing_symbol_at(node.start_byte(), &type_kinds)
                .map(|s| s.id);
            let key = (name.to_string(), class_id);
            if let Some(cached) = cache.get(&key) {
                return *cached;
            }
            let result = classify_definition_at_ident(uri, document, db, node)
                .map(|def| script_global_override(db, name, &def));
            cache.insert(key, result);
            result
        }
    }
}

fn is_member_access_rhs(node: Node, parent: Node) -> bool {
    if parent.kind() != kinds::MEMBER_ACCESS_EXPR {
        return false;
    }
    let mut cursor = parent.walk();
    let is_receiver = parent
        .named_children(&mut cursor)
        .next()
        .is_some_and(|c| c.id() == node.id());
    !is_receiver
}

fn classify_locally(node: Node, document: &ParsedDocument) -> Option<u32> {
    if let Some(parent) = node.parent()
        && is_member_access_rhs(node, parent)
    {
        return None;
    }

    let name = node.utf8_text(document.source.as_bytes()).ok()?;
    let byte_offset = node.start_byte();

    let callable_kinds = [SymbolKind::Function, SymbolKind::Method, SymbolKind::Event];
    if let Some(callable) = document
        .symbols
        .enclosing_symbol_at(byte_offset, &callable_kinds)
        && let Some(sym) = document
            .symbols
            .local_at_byte(callable.id, name, byte_offset)
    {
        return Some(symbol_kind_to_token_type(sym.kind));
    }

    let type_kinds = [SymbolKind::Class, SymbolKind::Struct, SymbolKind::State];
    if let Some(class) = document
        .symbols
        .enclosing_symbol_at(byte_offset, &type_kinds)
        && let Some(sym) = document.symbols.member_of(class.id, name).next()
    {
        return Some(symbol_kind_to_token_type(sym.kind));
    }

    document
        .symbols
        .top_level_by_name(name)
        .map(|sym| symbol_kind_to_token_type(sym.kind))
}

// redscripts.ini globals redirect to their class for Go-To-Def; for tokens, recolour
// the redirected class (or the synthetic INI Variable) as variable+defaultLibrary so
// `thePlayer` doesn't paint as a type. Workspace shadows win normally.
fn script_global_override(
    db: &SymbolDb,
    name: &str,
    def: &crate::resolve::Definition,
) -> (u32, u32) {
    let kind = def.symbol.kind;
    let Some(global_type) = db.script_global_type_name(name) else {
        return (symbol_kind_to_token_type(kind), 0);
    };
    let matches_global =
        kind == SymbolKind::Variable || (kind.is_instantiable() && def.symbol.name == global_type);
    if matches_global {
        (TT_VARIABLE, MOD_DEFAULT_LIBRARY)
    } else {
        (symbol_kind_to_token_type(kind), 0)
    }
}

fn symbol_kind_to_token_type(kind: SymbolKind) -> u32 {
    match kind {
        SymbolKind::Class | SymbolKind::NativeType | SymbolKind::Struct | SymbolKind::State => {
            TT_CLASS
        }
        SymbolKind::Enum => TT_ENUM,
        SymbolKind::EnumMember => TT_ENUM_MEMBER,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Event => TT_FUNCTION,
        SymbolKind::Field => TT_PROPERTY,
        SymbolKind::Variable => TT_VARIABLE,
        SymbolKind::Parameter => TT_PARAMETER,
    }
}

fn classify_anonymous_keyword(kind: &str) -> Option<u32> {
    match kind {
        // Control flow and constant.language keywords omitted - TextMate handles all of these.
        // Declaration and modifier keywords: introduce or modify a declaration.
        "class" | "struct" | "enum" | "state" | "function" | "event" | "extends" | "var"
        | "autobind" | "defaults" | "hint" | "abstract" | "statemachine" | "latent" | "import"
        | "const" | "final" | "editable" | "saved" | "optional" | "out" | "inlined" | "private"
        | "protected" | "public" | "cleanup" | "entry" | "exec" | "quest" | "reward"
        | "storyscene" | "timer" | "single" => Some(TT_MODIFIER),
        _ => None,
    }
}

fn encode(tokens: &[RawToken]) -> Vec<u32> {
    let mut encoded = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for token in tokens {
        let delta_line = token.line - prev_line;
        let delta_start = if delta_line == 0 {
            token.start_char - prev_start
        } else {
            token.start_char
        };

        encoded.push(delta_line);
        encoded.push(delta_start);
        encoded.push(token.length);
        encoded.push(token.token_type);
        encoded.push(token.token_modifiers);
        prev_line = token.line;
        prev_start = token.start_char;
    }

    encoded
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticTokenView {
    pub delta_line: u32,
    pub delta_start: u32,
    pub length: u32,
    pub token_type: u32,
    pub token_modifiers: u32,
}

#[cfg(any(test, feature = "test-support"))]
impl SemanticTokenView {
    pub fn token_type_name(&self) -> &'static str {
        TOKEN_TYPES
            .get(self.token_type as usize)
            .copied()
            .unwrap_or("?")
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn decode_tokens(encoded: &[u32]) -> Vec<SemanticTokenView> {
    encoded
        .chunks_exact(5)
        .map(|c| SemanticTokenView {
            delta_line: c[0],
            delta_start: c[1],
            length: c[2],
            token_type: c[3],
            token_modifiers: c[4],
        })
        .collect()
}

#[cfg(test)]
mod tests;
