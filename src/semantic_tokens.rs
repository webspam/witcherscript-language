use tree_sitter::Node;

use crate::line_index::LineIndex;
use crate::resolve::WorkspaceIndex;
use crate::symbols::{DocumentSymbols, SymbolKind};

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
const TT_KEYWORD: u32 = 7;
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
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
) -> Vec<u32> {
    let mut tokens: Vec<RawToken> = Vec::new();
    collect(
        root,
        source,
        line_index,
        symbols,
        workspace,
        base,
        &mut tokens,
    );
    encode(&tokens)
}

fn collect(
    node: Node,
    source: &str,
    line_index: &LineIndex,
    symbols: &DocumentSymbols,
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
    out: &mut Vec<RawToken>,
) {
    if let Some(token_type) = classify(node, source, symbols, workspace, base) {
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
            collect(child, source, line_index, symbols, workspace, base, out);
        }
        // Anonymous nodes with no classification (punctuation etc.) are silently skipped.
    }
}

fn classify(
    node: Node,
    source: &str,
    symbols: &DocumentSymbols,
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
) -> Option<u32> {
    match node.kind() {
        "ident" => classify_ident(node, source, symbols, workspace, base),
        "annotation_ident" => Some(TT_DECORATOR),
        "comment" => Some(TT_COMMENT),
        "literal_string" => Some(TT_STRING),
        // CName literals ('SomeName') are compile-time symbol references, not text.
        "literal_name" => Some(TT_ENUM_MEMBER),
        "literal_int" | "literal_float" | "literal_hex" => Some(TT_NUMBER),
        // Boolean/null/self keywords are named nodes that wrap a single anonymous keyword.
        "literal_bool" | "literal_null" => Some(TT_KEYWORD),
        "this_expr" | "super_expr" | "parent_expr" | "virtual_parent_expr" => Some(TT_KEYWORD),
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
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
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
        "type_annot" | "new_expr" => resolve_ident(node, source, symbols, workspace, base),
        // In `a.b`, both sides require resolution — never guess.
        "member_access_expr" => {
            let prev = node.prev_sibling();
            if prev.map(|n| n.kind() == ".").unwrap_or(false) {
                resolve_member_ident(node, source, symbols, workspace, base)
            } else {
                resolve_ident(node, source, symbols, workspace, base)
            }
        }
        // All other expression positions: only highlight if we can resolve the name.
        _ => resolve_ident(node, source, symbols, workspace, base),
    }
}

fn resolve_ident(
    node: Node,
    source: &str,
    symbols: &DocumentSymbols,
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
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

    // Workspace symbols from other files, then base scripts.
    if let Some(def) = workspace
        .find_top_level(name)
        .or_else(|| base.find_top_level(name))
    {
        return Some(symbol_kind_to_token_type(def.symbol.kind));
    }

    None
}

fn resolve_member_ident(
    node: Node,
    source: &str,
    symbols: &DocumentSymbols,
    workspace: &WorkspaceIndex,
    base: &WorkspaceIndex,
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

    workspace
        .find_member(&type_name, name)
        .or_else(|| base.find_member(&type_name, name))
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
        // Control flow and expression keywords.
        "if" | "else" | "while" | "for" | "do" | "switch" | "case" | "default" | "break"
        | "continue" | "return" | "new" | "delete" | "in" | "true" | "false" | "NULL" | "this"
        | "super" | "parent" | "virtual_parent" => Some(TT_KEYWORD),
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
mod tests {
    use tree_sitter::Parser;

    use super::collect_semantic_tokens;
    use crate::line_index::LineIndex;
    use crate::resolve::WorkspaceIndex;
    use crate::symbols::extract_symbols;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_witcherscript::language())
            .expect("failed to load WitcherScript grammar");
        parser.parse(source, None).expect("failed to parse source")
    }

    fn tokens_for(source: &str) -> Vec<u32> {
        tokens_for_with_workspace(source, &WorkspaceIndex::default())
    }

    fn tokens_for_with_workspace(source: &str, workspace: &WorkspaceIndex) -> Vec<u32> {
        let tree = parse(source);
        let index = LineIndex::new(source);
        let symbols = extract_symbols(tree.root_node(), source, &index);
        collect_semantic_tokens(
            tree.root_node(),
            source,
            &index,
            &symbols,
            workspace,
            &WorkspaceIndex::default(),
        )
    }

    #[test]
    fn emits_tokens_for_class_declaration() {
        // "class CExample {}" should produce at least keyword + class tokens
        let data = tokens_for("class CExample {}\n");
        // Each token is 5 u32 values; must have at least 2 tokens
        assert!(
            data.len() >= 10,
            "expected at least 2 tokens, got {}",
            data.len() / 5
        );
    }

    #[test]
    fn class_declaration_keyword_is_modifier() {
        // "class" is a declaration keyword → modifier, not control-flow keyword
        let source = "class CExample {}\n";
        let data = tokens_for(source);
        assert!(data.len() >= 5);
        assert_eq!(data[0], 0, "delta_line");
        assert_eq!(data[1], 0, "delta_start");
        assert_eq!(data[2], 5, "length of 'class'");
        assert_eq!(data[3], super::TT_MODIFIER, "token type should be modifier");
    }

    #[test]
    fn class_name_token_type_is_correct() {
        let source = "class CExample {}\n";
        let data = tokens_for(source);
        // Second token: delta_line=0, delta_start=6 (after 'class '), length=8 ("CExample"), type=TT_CLASS(0)
        assert!(data.len() >= 10);
        assert_eq!(data[5], 0, "second token delta_line");
        assert_eq!(data[6], 6, "second token delta_start (after 'class ')");
        assert_eq!(data[7], 8, "length of 'CExample'");
        assert_eq!(data[8], super::TT_CLASS, "token type should be class");
    }

    #[test]
    fn function_tokens_are_emitted() {
        let source = "function Foo() {}\n";
        let data = tokens_for(source);
        assert!(data.len() >= 10, "expected modifier + function name tokens");
        // 'function' declaration keyword → modifier
        assert_eq!(data[3], super::TT_MODIFIER);
        // 'Foo' name next — TT_FUNCTION
        assert_eq!(data[8], super::TT_FUNCTION);
    }

    #[test]
    fn specifier_is_modifier_not_keyword() {
        let source = "class C {\n private var x : int;\n}\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.contains(&super::TT_MODIFIER),
            "expected a modifier token for 'private', got types: {types:?}"
        );
    }

    #[test]
    fn var_is_modifier_not_keyword() {
        let source = "function F() { var x : int; }\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.contains(&super::TT_MODIFIER),
            "expected a modifier token for 'var', got types: {types:?}"
        );
    }

    #[test]
    fn control_flow_keywords_are_keyword_type() {
        let source = "function F() { if (true) { return; } }\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.contains(&super::TT_KEYWORD),
            "expected keyword tokens for 'if'/'true'/'return', got types: {types:?}"
        );
    }

    #[test]
    fn comment_token_type_is_correct() {
        let source = "// a comment\n";
        let data = tokens_for(source);
        assert!(data.len() >= 5);
        assert_eq!(data[3], super::TT_COMMENT);
    }

    #[test]
    fn string_literal_token_type_is_correct() {
        let source = "function F() { var s : string; s = \"hello\"; }\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.contains(&super::TT_STRING),
            "expected a string token, got types: {types:?}"
        );
    }

    #[test]
    fn name_literal_is_enum_member_not_string() {
        let source = "function F() { var n : CName; n = 'SomeName'; }\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.contains(&super::TT_ENUM_MEMBER),
            "expected enumMember token for name literal, got types: {types:?}"
        );
        assert!(
            !types.contains(&super::TT_STRING),
            "name literal should not be classified as string, got types: {types:?}"
        );
    }

    #[test]
    fn variable_use_gets_variable_token() {
        let source = "function F() { var x : int; x = 1; }\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.iter().filter(|&&t| t == super::TT_VARIABLE).count() >= 2,
            "expected variable token for both declaration and use of 'x', got types: {types:?}"
        );
    }

    #[test]
    fn member_access_lhs_gets_variable_token() {
        // Vector and its field X must be defined for the member access to resolve.
        let source =
            "struct Vector { var X : float; }\nfunction F() { var v : Vector; v.X = 0; }\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.iter().filter(|&&t| t == super::TT_VARIABLE).count() >= 2,
            "expected variable token for declaration and use of 'v', got types: {types:?}"
        );
        assert!(
            types.contains(&super::TT_PROPERTY),
            "expected property token for resolved field 'X', got types: {types:?}"
        );
    }

    #[test]
    fn unresolvable_type_annotation_gets_no_token() {
        // CObject is not defined — neither TT_CLASS nor any other token should appear for it.
        let source_with = "class CObject {}\nfunction F(x : CObject) {}\n";
        let source_without = "function F(x : CObject) {}\n";
        let types_with: Vec<u32> = tokens_for(source_with)
            .iter()
            .skip(3)
            .step_by(5)
            .copied()
            .collect();
        let types_without: Vec<u32> = tokens_for(source_without)
            .iter()
            .skip(3)
            .step_by(5)
            .copied()
            .collect();
        assert!(
            types_with.contains(&super::TT_CLASS),
            "defined CObject should produce a class token, got: {types_with:?}"
        );
        assert!(
            !types_without.contains(&super::TT_CLASS),
            "undefined CObject must not produce a class token, got: {types_without:?}"
        );
    }

    #[test]
    fn resolved_type_annotation_gets_class_token() {
        // CObject is defined — its use in a type annotation should resolve to TT_CLASS.
        let source = "class CObject {}\nfunction F(x : CObject) {}\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.contains(&super::TT_CLASS),
            "defined type in annotation should resolve to class token, got types: {types:?}"
        );
    }

    #[test]
    fn type_annotation_from_base_scripts_gets_class_token() {
        // CActor is defined only in base_scripts — the field type annotation should
        // still resolve and produce a class token.
        let base_source = "class CActor {}\n";
        let base_tree = parse(base_source);
        let base_index = LineIndex::new(base_source);
        let base_symbols = extract_symbols(base_tree.root_node(), base_source, &base_index);
        let mut base = super::WorkspaceIndex::default();
        base.update_document("file:///base/CActor.ws", &base_symbols);

        let source = "class SomeClass {\n  var actor : CActor;\n}\n";
        let tree = parse(source);
        let index = LineIndex::new(source);
        let symbols = extract_symbols(tree.root_node(), source, &index);
        let data = collect_semantic_tokens(
            tree.root_node(),
            source,
            &index,
            &symbols,
            &super::WorkspaceIndex::default(),
            &base,
        );
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.contains(&super::TT_CLASS),
            "CActor from base scripts must produce a class token, got types: {types:?}"
        );
    }
}
