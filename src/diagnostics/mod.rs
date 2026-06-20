use std::fmt::Write;
use std::ops::Range;
use std::path::Path;

use tree_sitter::{Node, Point};

use crate::line_index::SourceRange;

mod abstract_instantiation;
mod annotation_state_target;
mod arg_count;
mod base_script_conflict;
mod cst_walker;
mod duplicate_local;
mod duplicate_symbols;
mod inherited_field;
mod override_consistency;
mod parent_outside_state;
mod shadowing;
mod state_owner;
mod struct_temp_member;
mod super_field_access;
mod type_mismatch;
mod unknown_method;
mod unknown_symbol;
mod unused_symbol;
mod wrapped_method;

pub use abstract_instantiation::collect_abstract_instantiation_diagnostics;
pub use annotation_state_target::collect_annotation_state_target_diagnostics;
pub use arg_count::collect_arg_count_diagnostics;
pub use base_script_conflict::{
    KIND as BASE_SCRIPT_CONFLICT_KIND, basename_of, collect_base_script_conflict_diagnostics,
    relative_from_scripts,
};
pub(crate) use cst_walker::{
    CstRule, CstRuleCtx, ParallelRuleShard, access_is_inside_declaring_class,
    collect_nodes_with_error_subtree, collect_single_rule_diagnostics, declaring_class_of,
    run_parallel_pass, run_rules_on_document,
};
pub use duplicate_local::collect_duplicate_local_diagnostics;
pub use duplicate_symbols::collect_duplicate_symbol_diagnostics;
pub use inherited_field::collect_inherited_field_diagnostics;
pub use override_consistency::collect_override_consistency_diagnostics;
pub use parent_outside_state::collect_parent_outside_state_diagnostics;
pub use shadowing::collect_shadowing_diagnostics;
pub use state_owner::collect_state_owner_diagnostics;
pub use super_field_access::collect_super_field_access_diagnostics;
pub use type_mismatch::collect_type_mismatch_diagnostics;
pub use unknown_method::collect_unknown_method_diagnostics;
pub use unknown_symbol::collect_unknown_symbol_diagnostics;
pub use unused_symbol::{KIND as UNUSED_SYMBOL_KIND, collect_unused_symbol_diagnostics};
pub use wrapped_method::collect_wrapped_method_diagnostics;

use crate::cst::ancestors::find_ancestor_of_kind;
use crate::cst::walk::{CstVisitor, Visit, walk};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use abstract_instantiation::AbstractInstantiationRule;
use annotation_state_target::AnnotationStateTargetRule;
use arg_count::ArgCountRule;
use inherited_field::InheritedFieldRule;
use override_consistency::OverrideConsistencyRule;
use parent_outside_state::ParentOutsideStateRule;
use state_owner::StateOwnerRule;
use struct_temp_member::StructTempMemberRule;
use super_field_access::SuperFieldAccessRule;
use type_mismatch::TypeMismatchRule;
use unknown_method::UnknownMethodRule;
use unknown_symbol::run_unknown_symbol_parallel;
use unused_symbol::UnusedSymbolRule;
use wrapped_method::WrappedMethodRule;

pub fn collect_cst_diagnostics_for_document(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
) -> Vec<WorkspaceDiagnostic> {
    let method_rule = UnknownMethodRule;
    let wrapped_rule = WrappedMethodRule;
    let abstract_rule = AbstractInstantiationRule;
    let super_field_rule = SuperFieldAccessRule;
    let struct_temp_member_rule = StructTempMemberRule;
    let type_mismatch_rule = TypeMismatchRule;
    let arg_count_rule = ArgCountRule;
    let state_owner_rule = StateOwnerRule;
    let annotation_state_target_rule = AnnotationStateTargetRule;
    let inherited_field_rule = InheritedFieldRule;
    let override_consistency_rule = OverrideConsistencyRule;
    let parent_outside_state_rule = ParentOutsideStateRule;
    let unused_symbol_rule = UnusedSymbolRule;
    let rules: Vec<&dyn CstRule> = vec![
        &method_rule,
        &wrapped_rule,
        &abstract_rule,
        &super_field_rule,
        &struct_temp_member_rule,
        &type_mismatch_rule,
        &arg_count_rule,
        &state_owner_rule,
        &annotation_state_target_rule,
        &inherited_field_rule,
        &override_consistency_rule,
        &parent_outside_state_rule,
        &unused_symbol_rule,
    ];
    let mut diagnostics = run_rules_on_document(uri, document, db, &rules);

    let shard = run_unknown_symbol_parallel(uri, document, db);
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
    Hint,
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
    let mut syntax = SyntaxDiagnostics::new(source);
    walk(root, &mut syntax);
    syntax.finish()
}

pub fn format_tree(root: Node) -> String {
    let mut output = String::new();
    format_node(root, 0, &mut output);
    output
}

pub(crate) struct SyntaxDiagnostics<'s> {
    source: &'s str,
    diagnostics: Vec<ParseDiagnostic>,
}

impl<'s> SyntaxDiagnostics<'s> {
    pub(crate) fn new(source: &'s str) -> Self {
        Self {
            source,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn finish(self) -> Vec<ParseDiagnostic> {
        self.diagnostics
    }
}

impl<'tree> CstVisitor<'tree> for SyntaxDiagnostics<'_> {
    // Always descends: MISSING tokens are often anonymous and must still be seen.
    fn enter(&mut self, node: Node<'tree>) -> Visit {
        if node.is_error() || node.is_missing() {
            self.diagnostics
                .push(tree_error_diagnostic(node, self.source));
        }
        if node.kind() == kinds::INCOMPLETE_MEMBER_ACCESS_EXPR {
            self.diagnostics.push(syntax_diagnostic(
                node,
                self.source,
                "incomplete_member_access_expr",
                "Incomplete member access: expected identifier after '.'",
            ));
        }
        if node.kind() == kinds::TERNARY_COND_EXPR {
            self.diagnostics.push(syntax_diagnostic(
                node,
                self.source,
                "ternary_cond_expr",
                "Ternary expression is not supported: WitcherScript parses `cond ? a : b` \
                 but always evaluates it to 0 / false / void",
            ));
        }
        if node.kind() == kinds::LITERAL_STRING && self.source[node.byte_range()].contains('\n') {
            self.diagnostics.push(syntax_diagnostic(
                node,
                self.source,
                "string_linefeed",
                "String literals cannot contain a linefeed",
            ));
        }
        if matches!(node.kind(), kinds::LITERAL_INT | kinds::LITERAL_HEX)
            && int_literal_overflows(node.kind(), &self.source[node.byte_range()])
        {
            self.diagnostics.push(syntax_diagnostic(
                node,
                self.source,
                "int_overflow",
                "Integer literal overflows a 32-bit int",
            ));
        }
        if node.kind() == kinds::EVENT_DECL {
            collect_event_return_type(node, self.source, &mut self.diagnostics);
        }
        if node.kind() == kinds::RETURN_STMT && is_bare_return(node) && is_inside_event(node) {
            self.diagnostics.push(syntax_diagnostic(
                node,
                self.source,
                "event_bare_return",
                "Events return bool; a bare 'return;' cannot convert void to bool",
            ));
        }
        if matches!(
            node.kind(),
            kinds::MEMBER_DEFAULT_VAL | kinds::MEMBER_DEFAULT_VAL_BLOCK_ASSIGN
        ) {
            collect_non_constant_default(node, self.source, &mut self.diagnostics);
        }
        if node.kind() == kinds::FUNC_BLOCK {
            collect_late_local_vars_in_block(node, self.source, &mut self.diagnostics);
        }
        if node.kind() == kinds::STRUCT_DEF {
            collect_struct_prop_access_modifiers(node, self.source, &mut self.diagnostics);
        }
        Visit::Children
    }
}

fn syntax_diagnostic(node: Node, source: &str, kind: &str, message: &str) -> ParseDiagnostic {
    ParseDiagnostic {
        kind: kind.to_string(),
        message: message.to_string(),
        start: node.start_position(),
        end: node.end_position(),
        byte_range: node.start_byte()..node.end_byte(),
        snippet: line_snippet(source, node.start_position().row),
    }
}

fn int_literal_overflows(kind: &str, text: &str) -> bool {
    // Unparseable digit strings are too long for u64 and therefore overflow too.
    if kind == kinds::LITERAL_HEX {
        return u64::from_str_radix(&text[2..], 16)
            .ok()
            .is_none_or(|v| v > i32::MAX as u64);
    }
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    // The sign is part of the token, so -2147483648 is in range.
    let max = i32::MAX as u64 + u64::from(negative);
    digits.parse::<u64>().ok().is_none_or(|v| v > max)
}

fn collect_event_return_type(event: Node, source: &str, diagnostics: &mut Vec<ParseDiagnostic>) {
    let Some(return_type) = event.child_by_field_name(fields::RETURN_TYPE) else {
        return;
    };
    let text = &source[return_type.byte_range()];
    if text != "void" {
        diagnostics.push(syntax_diagnostic(
            return_type,
            source,
            "event_return_not_void",
            "An event's return type, if specified, must be void",
        ));
    }
}

fn is_bare_return(return_stmt: Node) -> bool {
    let mut cursor = return_stmt.walk();
    return_stmt
        .named_children(&mut cursor)
        .all(|child| child.kind() == kinds::COMMENT)
}

fn is_inside_event(node: Node) -> bool {
    find_ancestor_of_kind(node, &[kinds::EVENT_DECL, kinds::FUNC_DECL])
        .is_some_and(|ancestor| ancestor.kind() == kinds::EVENT_DECL)
}

fn collect_non_constant_default(
    default_val: Node,
    source: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) {
    let Some(value) = default_val.child_by_field_name(fields::VALUE) else {
        return;
    };
    if matches!(value.kind(), kinds::FUNC_CALL_EXPR | kinds::NEW_EXPR) {
        diagnostics.push(syntax_diagnostic(
            value,
            source,
            "non_constant_default",
            "'default' values must be compile-time constants; calls and 'new' are not",
        ));
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
        if !child.is_named() || matches!(child.kind(), kinds::COMMENT | kinds::NOP) {
            continue;
        }

        if child.kind() == kinds::LOCAL_VAR_DECL_STMT {
            if saw_code_statement {
                diagnostics.push(syntax_diagnostic(
                    child,
                    source,
                    "late_local_var_decl",
                    "Local variable declarations must precede executable statements",
                ));
            }
            continue;
        }

        saw_code_statement = true;
    }
}

fn collect_struct_prop_access_modifiers(
    struct_def: Node,
    source: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) {
    let mut cursor = struct_def.walk();
    for prop in struct_def.children(&mut cursor) {
        if prop.kind() != kinds::MEMBER_VAR_DECL {
            continue;
        }

        let mut prop_cursor = prop.walk();
        for specifier in prop.children(&mut prop_cursor) {
            if specifier.kind() != kinds::SPECIFIER {
                continue;
            }
            let keyword = &source[specifier.start_byte()..specifier.end_byte()];
            if matches!(keyword, "private" | "protected" | "public") {
                diagnostics.push(syntax_diagnostic(
                    specifier,
                    source,
                    "struct_property_access_modifier",
                    "Accessibility modifiers cannot be applied to struct properties",
                ));
            }
        }
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

    writeln!(
        output,
        "{}{}{} [{}:{}-{}:{}] bytes {}..{}",
        indent,
        node.kind(),
        marker,
        start.row + 1,
        start.column + 1,
        end.row + 1,
        end.column + 1,
        node.start_byte(),
        node.end_byte()
    )
    .unwrap();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        format_node(child, depth + 1, output);
    }
}

#[cfg(test)]
mod tests;
