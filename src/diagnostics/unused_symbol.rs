use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::resolve::{Definition, SymbolDb, find_references};
use crate::symbols::{AccessLevel, Symbol, SymbolKind};

use super::{CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic, collect_single_rule_diagnostics};

pub const KIND: &str = "unused_symbol";

pub(crate) struct UnusedSymbolRule;

impl CstRule for UnusedSymbolRule {
    fn name(&self) -> &'static str {
        "unused_symbol"
    }

    fn interested_in(&self, kind: &str) -> bool {
        matches!(
            kind,
            kinds::FUNC_PARAM_GROUP | kinds::LOCAL_VAR_DECL_STMT | kinds::MEMBER_VAR_DECL
        )
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_unused(node, ctx);
    }
}

pub fn collect_unused_symbol_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    collect_single_rule_diagnostics(&UnusedSymbolRule, documents, db)
}

fn check_unused(node: Node<'_>, ctx: &mut CstRuleCtx<'_, '_>) {
    // @addField injects a field into another class; its uses live in that class, not here.
    if node.child_by_field_name(fields::ANNOTATION).is_some() {
        return;
    }

    let type_field = if node.kind() == kinds::FUNC_PARAM_GROUP {
        fields::PARAM_TYPE
    } else {
        fields::VAR_TYPE
    };
    let type_end = node.child_by_field_name(type_field).map(|t| t.end_byte());
    let noun = match node.kind() {
        kinds::FUNC_PARAM_GROUP => "Parameter",
        kinds::LOCAL_VAR_DECL_STMT => "Local variable",
        _ => "Field",
    };

    let mut cursor = node.walk();
    let names: Vec<Node> = node
        .children_by_field_name(fields::NAMES, &mut cursor)
        .collect();
    // Grouped names share one type annotation, so a dim cannot reach the type without dimming siblings.
    let grouped = names.len() > 1;

    for ident in names {
        let Ok(name) = ident.utf8_text(ctx.document.source.as_bytes()) else {
            continue;
        };
        let Some(symbol) = declared_symbol(ctx.document, node, ident, name) else {
            continue;
        };
        // Only private fields are local enough to call unused; public/protected are the type's API.
        if symbol.kind == SymbolKind::Field && symbol.access != AccessLevel::Private {
            continue;
        }
        let definition = Definition {
            uri: ctx.uri.to_string(),
            symbol,
        };
        let referenced = !find_references(
            &definition,
            ctx.document,
            &[(ctx.uri, ctx.document)],
            ctx.db,
            false,
        )
        .is_empty();
        if referenced {
            continue;
        }

        let end = if grouped {
            ident.end_byte()
        } else {
            type_end.unwrap_or_else(|| ident.end_byte())
        };
        let range = ctx.document.line_index.byte_range_to_range(
            &ctx.document.source,
            ident.start_byte(),
            end,
        );
        ctx.diagnostics.push(WorkspaceDiagnostic {
            kind: KIND.to_string(),
            message: format!("{noun} '{name}' is never used"),
            severity: Severity::Hint,
            range,
            related: Vec::new(),
            data: None,
        });
    }
}

fn declared_symbol(
    document: &ParsedDocument,
    node: Node<'_>,
    ident: Node<'_>,
    name: &str,
) -> Option<Symbol> {
    let symbols = &document.symbols;
    let ident_start = ident.start_byte();
    if node.kind() == kinds::MEMBER_VAR_DECL {
        let class = symbols.enclosing_symbol_at(
            node.start_byte(),
            &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
        )?;
        symbols
            .member_of(class.id, name)
            .find(|s| s.selection_byte_range.start == ident_start)
            .cloned()
    } else {
        let func = symbols.enclosing_symbol_at(
            node.start_byte(),
            &[SymbolKind::Function, SymbolKind::Method, SymbolKind::Event],
        )?;
        symbols.local_at_byte(func.id, name, ident_start).cloned()
    }
}

#[cfg(test)]
mod tests;
