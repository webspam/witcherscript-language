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
    collect_from_node(root, source, &mut diagnostics);
    diagnostics
}

pub fn format_tree(root: Node) -> String {
    let mut output = String::new();
    format_node(root, 0, &mut output);
    output
}

fn collect_from_node(node: Node, source: &str, diagnostics: &mut Vec<ParseDiagnostic>) {
    if node.is_error() || node.is_missing() {
        diagnostics.push(diagnostic_for_node(node, source));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_from_node(child, source, diagnostics);
    }
}

fn diagnostic_for_node(node: Node, source: &str) -> ParseDiagnostic {
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
