use tree_sitter::Node;

use crate::line_index::LineIndex;

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
const TT_TYPE: u32 = 11;
const TT_DECORATOR: u32 = 12;

struct RawToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
}

pub fn collect_semantic_tokens(root: Node, source: &str, line_index: &LineIndex) -> Vec<u32> {
    let mut tokens: Vec<RawToken> = Vec::new();
    collect(root, source, line_index, &mut tokens);
    encode(&tokens)
}

fn collect(node: Node, source: &str, line_index: &LineIndex, out: &mut Vec<RawToken>) {
    if let Some(token_type) = classify(node) {
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
            collect(child, source, line_index, out);
        }
        // Anonymous nodes with no classification (punctuation etc.) are silently skipped.
    }
}

fn classify(node: Node) -> Option<u32> {
    match node.kind() {
        "ident" => classify_ident(node),
        "annotation_ident" => Some(TT_DECORATOR),
        "comment" => Some(TT_COMMENT),
        "literal_string" | "literal_name" => Some(TT_STRING),
        "literal_int" | "literal_float" | "literal_hex" => Some(TT_NUMBER),
        // Boolean/null/self keywords are named nodes that wrap a single anonymous keyword.
        "literal_bool" | "literal_null" => Some(TT_KEYWORD),
        "this_expr" | "super_expr" | "parent_expr" | "virtual_parent_expr" => Some(TT_KEYWORD),
        // Specifiers (public/private/editable/saved/…) and function-flavour keywords
        // (entry/exec/quest/…) are named nodes wrapping a single anonymous keyword.
        "specifier" | "func_flavour" | "autobind_single" => Some(TT_KEYWORD),
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

fn classify_ident(node: Node) -> Option<u32> {
    let parent = node.parent()?;
    match parent.kind() {
        "class_decl" | "struct_decl" | "state_decl" => Some(TT_CLASS),
        "enum_decl" => Some(TT_ENUM),
        "enum_decl_variant" => Some(TT_ENUM_MEMBER),
        "func_decl" | "event_decl" => Some(TT_FUNCTION),
        "func_param_group" => Some(TT_PARAMETER),
        "member_var_decl" => Some(TT_PROPERTY),
        "local_var_decl_stmt" => Some(TT_VARIABLE),
        "autobind_decl" => Some(TT_PROPERTY),
        "type_annot" => Some(TT_TYPE),
        // The class name in `new ClassName` is a type reference.
        "new_expr" => Some(TT_TYPE),
        // In `a.b`, the member ident (after `.`) is a property reference.
        "member_access_expr" => {
            let prev = node.prev_sibling();
            if prev.map(|n| n.kind() == ".").unwrap_or(false) {
                Some(TT_PROPERTY)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn classify_anonymous_keyword(kind: &str) -> Option<u32> {
    match kind {
        "class" | "struct" | "enum" | "state" | "function" | "event" | "var" | "return" | "if"
        | "else" | "while" | "for" | "do" | "switch" | "case" | "default" | "break"
        | "continue" | "new" | "delete" | "in" | "extends" | "defaults" | "hint" | "autobind"
        | "abstract" | "statemachine" | "latent" | "import" | "const" | "final" | "editable"
        | "saved" | "optional" | "out" | "inlined" | "private" | "protected" | "public"
        | "cleanup" | "entry" | "exec" | "quest" | "reward" | "storyscene" | "timer" | "true"
        | "false" | "NULL" | "this" | "super" | "parent" | "virtual_parent" | "single" => {
            Some(TT_KEYWORD)
        }
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

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_witcherscript::language())
            .expect("failed to load WitcherScript grammar");
        parser.parse(source, None).expect("failed to parse source")
    }

    fn tokens_for(source: &str) -> Vec<u32> {
        let tree = parse(source);
        let index = LineIndex::new(source);
        collect_semantic_tokens(tree.root_node(), source, &index)
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
    fn keyword_token_type_is_correct() {
        // "class" keyword on line 0 should be the first token
        let source = "class CExample {}\n";
        let data = tokens_for(source);
        // First token: deltaLine=0, deltaStart=0, length=5, type=TT_KEYWORD(7)
        assert!(data.len() >= 5);
        assert_eq!(data[0], 0, "delta_line");
        assert_eq!(data[1], 0, "delta_start");
        assert_eq!(data[2], 5, "length of 'class'");
        assert_eq!(data[3], super::TT_KEYWORD, "token type should be keyword");
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
        assert!(data.len() >= 10, "expected keyword + function name tokens");
        // 'function' keyword first
        assert_eq!(data[3], super::TT_KEYWORD);
        // 'Foo' name next — TT_FUNCTION
        assert_eq!(data[8], super::TT_FUNCTION);
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
    fn type_annotation_ident_is_typed() {
        let source = "function F(x : CObject) {}\n";
        let data = tokens_for(source);
        let types: Vec<u32> = data.iter().skip(3).step_by(5).copied().collect();
        assert!(
            types.contains(&super::TT_TYPE),
            "expected a type token for 'CObject', got types: {types:?}"
        );
    }
}
