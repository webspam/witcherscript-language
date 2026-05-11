use tree_sitter::Node;

use crate::line_index::LineIndex;
use crate::resolve::SymbolDb;
use crate::symbols::{AccessLevel, DocumentSymbols, SymbolKind};

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

pub fn collect_semantic_tokens(
    root: Node,
    source: &str,
    line_index: &LineIndex,
    symbols: &DocumentSymbols,
    db: &SymbolDb,
) -> Vec<u32> {
    let mut tokens: Vec<RawToken> = Vec::new();
    collect(root, source, line_index, symbols, db, &mut tokens);
    encode(&tokens)
}

fn collect(
    node: Node,
    source: &str,
    line_index: &LineIndex,
    symbols: &DocumentSymbols,
    db: &SymbolDb,
    out: &mut Vec<RawToken>,
) {
    if let Some(token_type) = classify(node, source, symbols, db) {
        let range = line_index.byte_range_to_range(source, node.start_byte(), node.end_byte());
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
            collect(child, source, line_index, symbols, db, out);
        }
        // Anonymous nodes with no classification (punctuation etc.) are silently skipped.
    }
}

fn classify(node: Node, source: &str, symbols: &DocumentSymbols, db: &SymbolDb) -> Option<u32> {
    match node.kind() {
        "ident" => classify_ident(node, source, symbols, db),
        "annotation_ident" => Some(TT_DECORATOR),
        "comment" => Some(TT_COMMENT),
        "literal_string" => Some(TT_STRING),
        // CName literals ('SomeName') are compile-time symbol references, not text.
        "literal_name" => Some(TT_ENUM_MEMBER),
        "literal_int" | "literal_float" | "literal_hex" => Some(TT_NUMBER),
        // literal_bool, literal_null, this_expr etc. are omitted — TextMate constant.language wins.
        // Specifiers (public/private/editable/saved/…) are access/storage modifiers.
        "specifier" => Some(TT_MODIFIER),
        // Function-flavour keywords (entry/exec/quest/…) modify the function declaration.
        "func_flavour" | "autobind_single" => Some(TT_MODIFIER),
        _ => {
            // Anonymous nodes whose kind string IS the keyword text.
            if !node.is_named() {
                classify_anonymous_keyword(node.kind())
            } else {
                None // named container node — recurse into children
            }
        }
    }
}

fn classify_ident(
    node: Node,
    source: &str,
    symbols: &DocumentSymbols,
    db: &SymbolDb,
) -> Option<u32> {
    let parent = node.parent()?;
    match parent.kind() {
        // Declaration sites — syntactically unambiguous.
        "class_decl" | "struct_decl" | "state_decl" => Some(TT_CLASS),
        "enum_decl" => Some(TT_ENUM),
        "enum_decl_variant" => Some(TT_ENUM_MEMBER),
        "func_decl" | "event_decl" => Some(TT_FUNCTION),
        "func_param_group" => Some(TT_PARAMETER),
        "member_var_decl" => Some(TT_PROPERTY),
        "local_var_decl_stmt" => Some(TT_VARIABLE),
        "autobind_decl" => Some(TT_PROPERTY),
        // Type annotations and `new ClassName` — only highlight if the type resolves.
        "type_annot" | "new_expr" => resolve_ident(node, source, symbols, db),
        // In `a.b`, both sides require resolution — never guess.
        "member_access_expr" => {
            let prev = node.prev_sibling();
            if prev.map(|n| n.kind() == ".").unwrap_or(false) {
                resolve_member_ident(node, source, symbols, db)
            } else {
                resolve_ident(node, source, symbols, db)
            }
        }
        // All other expression positions: only highlight if we can resolve the name.
        _ => resolve_ident(node, source, symbols, db),
    }
}

fn resolve_ident(
    node: Node,
    source: &str,
    symbols: &DocumentSymbols,
    db: &SymbolDb,
) -> Option<u32> {
    let name = node.utf8_text(source.as_bytes()).ok()?;
    let byte_offset = node.start_byte();

    // Local variables and parameters in the enclosing function.
    let enclosing_fn = symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
    );
    if let Some(fn_sym) = enclosing_fn {
        if let Some(local) = symbols
            .children_of(Some(fn_sym.id))
            .filter(|s| {
                matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter)
                    && s.name == name
                    && s.selection_byte_range.start <= byte_offset
            })
            .max_by_key(|s| s.selection_byte_range.start)
        {
            return Some(symbol_kind_to_token_type(local.kind));
        }
    }

    // Members of the enclosing class.
    let enclosing_class = symbols.enclosing_symbol_at(
        byte_offset,
        &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
    );
    if let Some(class_sym) = enclosing_class {
        if let Some(member) = symbols.children_of(Some(class_sym.id)).find(|s| {
            s.name == name
                && !matches!(
                    s.kind,
                    SymbolKind::Variable
                        | SymbolKind::Parameter
                        | SymbolKind::Function
                        | SymbolKind::Method
                        | SymbolKind::Event
                )
        }) {
            return Some(symbol_kind_to_token_type(member.kind));
        }
        if let Some(method) = symbols
            .children_of(Some(class_sym.id))
            .find(|s| s.name == name && matches!(s.kind, SymbolKind::Method | SymbolKind::Event))
        {
            return Some(symbol_kind_to_token_type(method.kind));
        }
    }

    // Document top-level symbols.
    if let Some(top) = symbols.children_of(None).find(|s| s.name == name) {
        return Some(symbol_kind_to_token_type(top.kind));
    }

    if let Some(def) = db.find_top_level(name) {
        return Some(symbol_kind_to_token_type(def.symbol.kind));
    }

    None
}

fn resolve_member_ident(
    node: Node,
    source: &str,
    symbols: &DocumentSymbols,
    db: &SymbolDb,
) -> Option<u32> {
    let name = node.utf8_text(source.as_bytes()).ok()?;
    let parent = node.parent()?;

    let mut cursor = parent.walk();
    let receiver = parent.named_children(&mut cursor).next()?;

    let type_name: String = match receiver.kind() {
        "this_expr" => symbols
            .enclosing_symbol_at(
                node.start_byte(),
                &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
            )?
            .name
            .clone(),
        "super_expr" | "parent_expr" | "virtual_parent_expr" => {
            let class_sym = symbols.enclosing_symbol_at(
                node.start_byte(),
                &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
            )?;
            class_sym
                .detail
                .as_deref()?
                .strip_prefix("extends ")?
                .to_string()
        }
        "ident" => {
            let receiver_name = receiver.utf8_text(source.as_bytes()).ok()?;
            let enclosing_fn = symbols.enclosing_symbol_at(
                node.start_byte(),
                &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
            )?;
            symbols
                .children_of(Some(enclosing_fn.id))
                .filter(|s| {
                    matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter)
                        && s.name == receiver_name
                        && s.selection_byte_range.start <= node.start_byte()
                })
                .max_by_key(|s| s.selection_byte_range.start)
                .and_then(|s| s.type_annotation.clone())?
        }
        _ => return None,
    };

    let container = symbols.all().iter().find(|s| {
        s.name == type_name
            && matches!(
                s.kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
            )
    });
    if let Some(c) = container {
        if let Some(member) = symbols.children_of(Some(c.id)).find(|s| s.name == name) {
            return Some(symbol_kind_to_token_type(member.kind));
        }
    }

    db.find_member(&type_name, name, AccessLevel::Public)
        .map(|def| symbol_kind_to_token_type(def.symbol.kind))
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
