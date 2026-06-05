use std::ops::Range;
use std::path::Path;

use tree_sitter::{Node, Point};

use crate::line_index::SourceRange;

mod abstract_instantiation;
mod base_script_conflict;
mod cst_walker;
mod duplicate_local;
mod duplicate_symbols;
mod shadowing;
mod super_field_access;
mod type_mismatch;
mod unknown_method;
mod unknown_symbol;
mod wrapped_method;

pub use abstract_instantiation::collect_abstract_instantiation_diagnostics;
pub use base_script_conflict::{
    basename_of, collect_base_script_conflict_diagnostics, relative_from_scripts,
    KIND as BASE_SCRIPT_CONFLICT_KIND,
};
pub use cst_walker::PassMode;
pub(crate) use cst_walker::{
    access_is_inside_declaring_class, collect_nodes_with_error_subtree, declaring_class_of,
    run_pass, run_rules_on_document, CstRule, CstRuleCtx, ParallelRuleShard,
};
pub use duplicate_local::collect_duplicate_local_diagnostics;
pub use duplicate_symbols::collect_duplicate_symbol_diagnostics;
pub use shadowing::collect_shadowing_diagnostics;
pub use super_field_access::collect_super_field_access_diagnostics;
pub use type_mismatch::collect_type_mismatch_diagnostics;
pub use unknown_method::collect_unknown_method_diagnostics;
pub use unknown_symbol::collect_unknown_symbol_diagnostics;
pub use wrapped_method::collect_wrapped_method_diagnostics;

use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use abstract_instantiation::AbstractInstantiationRule;
use super_field_access::SuperFieldAccessRule;
use type_mismatch::TypeMismatchRule;
use unknown_method::UnknownMethodRule;
use unknown_symbol::run_unknown_symbol;
use wrapped_method::WrappedMethodRule;

pub fn collect_cst_diagnostics_for_document(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    unknown_pass: PassMode,
) -> Vec<WorkspaceDiagnostic> {
    let method_rule = UnknownMethodRule;
    let wrapped_rule = WrappedMethodRule;
    let abstract_rule = AbstractInstantiationRule;
    let super_field_rule = SuperFieldAccessRule;
    let type_mismatch_rule = TypeMismatchRule;
    let rules: Vec<&dyn CstRule> = vec![
        &method_rule,
        &wrapped_rule,
        &abstract_rule,
        &super_field_rule,
        &type_mismatch_rule,
    ];
    let mut diagnostics = run_rules_on_document(uri, document, db, &rules);

    let shard = run_unknown_symbol(uri, document, db, unknown_pass);
    db.merge_observations(shard.observer);
    diagnostics.extend(shard.diagnostics);
    diagnostics.sort_by(|a, b| {
        (a.range.start.line, a.range.start.character)
            .cmp(&(b.range.start.line, b.range.start.character))
    });
    diagnostics
}

#[derive(Debug, Clone)]
pub struct ParseDiagnostic {
    pub kind: String,
    pub message: String,
    pub start: Point,
    pub end: Point,
    pub byte_range: Range<usize>,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Severity {
    #[default]
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct WorkspaceDiagnostic {
    pub kind: String,
    pub message: String,
    pub severity: Severity,
    pub range: SourceRange,
    pub related: Vec<RelatedLocation>,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct RelatedLocation {
    pub uri: String,
    pub range: SourceRange,
    pub message: String,
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
    collect_walk(root, source, &mut diagnostics);
    diagnostics
}

pub fn format_tree(root: Node) -> String {
    let mut output = String::new();
    format_node(root, 0, &mut output);
    output
}

fn collect_walk(node: Node, source: &str, diagnostics: &mut Vec<ParseDiagnostic>) {
    if node.is_error() || node.is_missing() {
        diagnostics.push(tree_error_diagnostic(node, source));
    }
    if node.kind() == "incomplete_member_access_expr" {
        diagnostics.push(incomplete_member_access_diagnostic(node, source));
    }
    if node.kind() == "ternary_cond_expr" {
        diagnostics.push(ternary_expr_diagnostic(node, source));
    }
    if node.kind() == "func_block" {
        collect_late_local_vars_in_block(node, source, diagnostics);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_walk(child, source, diagnostics);
    }
}

fn incomplete_member_access_diagnostic(node: Node, source: &str) -> ParseDiagnostic {
    ParseDiagnostic {
        kind: "incomplete_member_access_expr".to_string(),
        message: "Incomplete member access: expected identifier after '.'".to_string(),
        start: node.start_position(),
        end: node.end_position(),
        byte_range: node.start_byte()..node.end_byte(),
        snippet: line_snippet(source, node.start_position().row),
    }
}

fn ternary_expr_diagnostic(node: Node, source: &str) -> ParseDiagnostic {
    ParseDiagnostic {
        kind: "ternary_cond_expr".to_string(),
        message: "Ternary expression is not supported: WitcherScript parses `cond ? a : b` \
                  but always evaluates it to 0 / false / void"
            .to_string(),
        start: node.start_position(),
        end: node.end_position(),
        byte_range: node.start_byte()..node.end_byte(),
        snippet: line_snippet(source, node.start_position().row),
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
        format!("Missing {}", node.kind())
    } else {
        "Syntax error".to_string()
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
        message: "Local variable declarations must precede executable statements".to_string(),
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
mod tests;
