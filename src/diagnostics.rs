use std::ops::Range;
use std::path::Path;

use tree_sitter::{Node, Point};

#[derive(Debug, Clone)]
pub struct ParseDiagnostic {
    pub kind: String,
    pub message: String,
    pub start: Point,
    pub end: Point,
    pub byte_range: Range<usize>,
    pub snippet: Option<String>,
}

impl ParseDiagnostic {
    pub fn display(&self, path: &Path) -> String {
        let line = self.start.row + 1;
        let column = self.start.column + 1;
        let mut output = format!(
            "{}:{}:{}: {} ({}, end {}:{}, bytes {}..{})",
            path.display(),
            line,
            column,
            self.message,
            self.kind,
            self.end.row + 1,
            self.end.column + 1,
            self.byte_range.start,
            self.byte_range.end
        );

        if let Some(snippet) = &self.snippet {
            output.push('\n');
            output.push_str("  ");
            output.push_str(snippet.trim_end());
        }

        output
    }
}

pub fn collect_diagnostics(root: Node, source: &str) -> Vec<ParseDiagnostic> {
    let mut diagnostics = Vec::new();
    collect_tree_errors(root, source, &mut diagnostics);
    collect_incomplete_exprs(root, source, &mut diagnostics);
    collect_late_local_vars(root, source, &mut diagnostics);
    diagnostics
}

pub fn format_tree(root: Node) -> String {
    let mut output = String::new();
    format_node(root, 0, &mut output);
    output
}

fn collect_tree_errors(node: Node, source: &str, diagnostics: &mut Vec<ParseDiagnostic>) {
    if node.is_error() || node.is_missing() {
        diagnostics.push(tree_error_diagnostic(node, source));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_errors(child, source, diagnostics);
    }
}

fn collect_incomplete_exprs(node: Node, source: &str, diagnostics: &mut Vec<ParseDiagnostic>) {
    if node.kind() == "incomplete_member_access_expr" {
        diagnostics.push(ParseDiagnostic {
            kind: "incomplete_member_access_expr".to_string(),
            message: "incomplete member access: expected identifier after '.'".to_string(),
            start: node.start_position(),
            end: node.end_position(),
            byte_range: node.start_byte()..node.end_byte(),
            snippet: line_snippet(source, node.start_position().row),
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_incomplete_exprs(child, source, diagnostics);
    }
}

fn collect_late_local_vars(node: Node, source: &str, diagnostics: &mut Vec<ParseDiagnostic>) {
    if node.kind() == "func_block" {
        collect_late_local_vars_in_block(node, source, diagnostics);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_late_local_vars(child, source, diagnostics);
    }
}

fn collect_late_local_vars_in_block(
    block: Node,
    source: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) {
    let mut saw_code_statement = false;
    let mut cursor = block.walk();

    for child in block.children(&mut cursor) {
        if !child.is_named() || matches!(child.kind(), "comment" | "nop") {
            continue;
        }

        if child.kind() == "local_var_decl_stmt" {
            if saw_code_statement {
                diagnostics.push(late_local_var_diagnostic(child, source));
            }
            continue;
        }

        saw_code_statement = true;
    }
}

fn tree_error_diagnostic(node: Node, source: &str) -> ParseDiagnostic {
    let kind = node.kind().to_string();
    let message = if node.is_missing() {
        format!("missing {}", node.kind())
    } else {
        "syntax error".to_string()
    };

    ParseDiagnostic {
        kind,
        message,
        start: node.start_position(),
        end: node.end_position(),
        byte_range: node.start_byte()..node.end_byte(),
        snippet: line_snippet(source, node.start_position().row),
    }
}

fn late_local_var_diagnostic(node: Node, source: &str) -> ParseDiagnostic {
    ParseDiagnostic {
        kind: "late_local_var_decl".to_string(),
        message: "local variable declarations must precede executable statements".to_string(),
        start: node.start_position(),
        end: node.end_position(),
        byte_range: node.start_byte()..node.end_byte(),
        snippet: line_snippet(source, node.start_position().row),
    }
}

fn line_snippet(source: &str, row: usize) -> Option<String> {
    source.lines().nth(row).map(str::to_string)
}

fn format_node(node: Node, depth: usize, output: &mut String) {
    let indent = "  ".repeat(depth);
    let start = node.start_position();
    let end = node.end_position();
    let marker = if node.is_error() {
        " ERROR"
    } else if node.is_missing() {
        " MISSING"
    } else {
        ""
    };

    output.push_str(&format!(
        "{}{}{} [{}:{}-{}:{}] bytes {}..{}\n",
        indent,
        node.kind(),
        marker,
        start.row + 1,
        start.column + 1,
        end.row + 1,
        end.column + 1,
        node.start_byte(),
        node.end_byte()
    ));

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        format_node(child, depth + 1, output);
    }
}

#[cfg(test)]
mod tests {
    use super::collect_diagnostics;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_witcherscript::language())
            .expect("failed to load WitcherScript grammar");
        parser.parse(source, None).expect("failed to parse source")
    }

    #[test]
    fn accepts_local_vars_before_statements() {
        let source = "function Ok() {\n var a : int;\n // comment\n a = 1;\n}\n";
        let tree = parse(source);

        let diagnostics = collect_diagnostics(tree.root_node(), source);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn reports_local_vars_after_statements() {
        let source = "function Bad() {\n a = 1;\n // comment\n var b : int;\n}\n";
        let tree = parse(source);

        let diagnostics = collect_diagnostics(tree.root_node(), source);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "late_local_var_decl");
    }

    #[test]
    fn reports_incomplete_member_access() {
        let source = "class C extends CR4Player {\n  var x : W3AbilityManager;\n  function F() {\n    x = super.\n  }\n}\n";
        let tree = parse(source);

        let diagnostics = collect_diagnostics(tree.root_node(), source);

        let incomplete = diagnostics
            .iter()
            .find(|d| d.kind == "incomplete_member_access_expr");
        assert!(
            incomplete.is_some(),
            "expected incomplete_member_access_expr diagnostic"
        );
        let d = incomplete.unwrap();
        assert_eq!(d.start.row, 3);
        assert_eq!(d.start.row, d.end.row);
    }
}
