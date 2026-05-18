use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::{classify_definition_at_ident, SymbolDb};
use crate::symbols::SymbolKind;

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

pub const TOKEN_MODIFIERS: &[&str] = &["declaration"];

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

struct RawToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
}

pub fn collect_semantic_tokens(uri: &str, document: &ParsedDocument, db: &SymbolDb) -> Vec<u32> {
    let mut tokens: Vec<RawToken> = Vec::new();
    collect(document.tree.root_node(), uri, document, db, &mut tokens);
    encode(&tokens)
}

fn collect(
    node: Node,
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    out: &mut Vec<RawToken>,
) {
    if let Some(token_type) = classify(node, uri, document, db) {
        let range = document.line_index.byte_range_to_range(
            &document.source,
            node.start_byte(),
            node.end_byte(),
        );
        if range.start.line == range.end.line && range.end.character > range.start.character {
            out.push(RawToken {
                line: range.start.line,
                start_char: range.start.character,
                length: range.end.character - range.start.character,
                token_type,
            });
        }
        // Don't recurse — this node's full span is covered.
    } else if node.is_named() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect(child, uri, document, db, out);
        }
    }
}

fn classify(node: Node, uri: &str, document: &ParsedDocument, db: &SymbolDb) -> Option<u32> {
    match node.kind() {
        "ident" => classify_ident(node, uri, document, db),
        "annotation_ident" => Some(TT_DECORATOR),
        "comment" => Some(TT_COMMENT),
        // CName literals ('SomeName') are compile-time symbol references, not text.
        "literal_name" => Some(TT_ENUM_MEMBER),
        "literal_string" => Some(TT_STRING),
        "literal_int" | "literal_float" | "literal_hex" => Some(TT_NUMBER),
        // literal_bool, literal_null, this_expr etc. are omitted — TextMate constant.language wins.
        "specifier" | "func_flavour" | "autobind_single" => Some(TT_MODIFIER),
        _ => {
            if !node.is_named() {
                classify_anonymous_keyword(node.kind())
            } else {
                None
            }
        }
    }
}

fn classify_ident(node: Node, uri: &str, document: &ParsedDocument, db: &SymbolDb) -> Option<u32> {
    let parent = node.parent()?;
    match parent.kind() {
        "class_decl" | "struct_decl" | "state_decl" => Some(TT_CLASS),
        "enum_decl" => Some(TT_ENUM),
        "enum_decl_variant" => Some(TT_ENUM_MEMBER),
        "func_decl" | "event_decl" => Some(TT_FUNCTION),
        "func_param_group" => Some(TT_PARAMETER),
        "member_var_decl" | "autobind_decl" => Some(TT_PROPERTY),
        "local_var_decl_stmt" => Some(TT_VARIABLE),
        _ => classify_locally(node, document).or_else(|| {
            classify_definition_at_ident(uri, document, db, node)
                .map(|def| symbol_kind_to_token_type(def.symbol.kind))
        }),
    }
}

fn classify_locally(node: Node, document: &ParsedDocument) -> Option<u32> {
    if let Some(parent) = node.parent() {
        if parent.kind() == "member_access_expr" {
            let mut cursor = parent.walk();
            let is_receiver = parent
                .named_children(&mut cursor)
                .next()
                .map(|c| c.id() == node.id())
                .unwrap_or(false);
            if !is_receiver {
                return None;
            }
        }
    }

    let name = node.utf8_text(document.source.as_bytes()).ok()?;
    let byte_offset = node.start_byte();

    let callable_kinds = [SymbolKind::Function, SymbolKind::Method, SymbolKind::Event];
    if let Some(callable) = document
        .symbols
        .enclosing_symbol_at(byte_offset, &callable_kinds)
    {
        if let Some(sym) = document
            .symbols
            .local_at_byte(callable.id, name, byte_offset)
        {
            return Some(symbol_kind_to_token_type(sym.kind));
        }
    }

    let type_kinds = [SymbolKind::Class, SymbolKind::Struct, SymbolKind::State];
    if let Some(class) = document
        .symbols
        .enclosing_symbol_at(byte_offset, &type_kinds)
    {
        if let Some(sym) = document.symbols.member_of(class.id, name).next() {
            return Some(symbol_kind_to_token_type(sym.kind));
        }
    }

    document
        .symbols
        .top_level_by_name(name)
        .map(|sym| symbol_kind_to_token_type(sym.kind))
}

fn symbol_kind_to_token_type(kind: SymbolKind) -> u32 {
    match kind {
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::State => TT_CLASS,
        SymbolKind::Enum => TT_ENUM,
        SymbolKind::EnumVariant => TT_ENUM_MEMBER,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Event => TT_FUNCTION,
        SymbolKind::Field => TT_PROPERTY,
        SymbolKind::Variable => TT_VARIABLE,
        SymbolKind::Parameter => TT_PARAMETER,
    }
}

fn classify_anonymous_keyword(kind: &str) -> Option<u32> {
    match kind {
        // Control flow and constant.language keywords omitted — TextMate handles all of these.
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
        encoded.push(0); // no modifiers
        prev_line = token.line;
        prev_start = token.start_char;
    }

    encoded
}

#[cfg(test)]
mod tests;
