use std::collections::HashMap;

use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use crate::symbols::SymbolKind;

use super::{
    CstRule, CstRuleCtx, RelatedLocation, Severity, WorkspaceDiagnostic,
    collect_single_rule_diagnostics,
};

pub const KIND_WEAKER_ACCESS: &str = "override_weaker_access";
pub const KIND_PARAM_COUNT: &str = "override_param_count";

pub(crate) struct OverrideConsistencyRule;

impl CstRule for OverrideConsistencyRule {
    fn name(&self) -> &'static str {
        "override_consistency"
    }

    fn interested_in(&self, kind: &str) -> bool {
        kind == kinds::FUNC_DECL
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        if ctx.in_error_subtree {
            return;
        }
        check_override(node, ctx);
    }
}

pub fn collect_override_consistency_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    collect_single_rule_diagnostics(&OverrideConsistencyRule, documents, db)
}

fn check_override<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    if node.child_by_field_name(fields::ANNOTATION).is_some() {
        return None;
    }
    let container = ctx
        .document
        .symbols
        .enclosing_symbol_at(node.start_byte(), &[SymbolKind::Class, SymbolKind::State])?;
    let name_ident = node.child_by_field_name(fields::NAME)?;
    // safe: ident nodes are sliced from a UTF-8 String on char-aligned boundaries
    let name = name_ident.utf8_text(ctx.document.source.as_bytes()).ok()?;
    let parent = ctx.db.superclass_of(&container.name)?;
    let ancestor = ctx.db.find_class_body_member(&parent, name)?;
    if ancestor.symbol.kind != SymbolKind::Method {
        return None;
    }
    let own = ctx
        .document
        .symbols
        .enclosing_symbol_at(name_ident.start_byte(), &[SymbolKind::Method])?;

    if own.access < ancestor.symbol.access {
        let ancestor_class = ancestor.symbol.container_name.as_deref().unwrap_or(&parent);
        let range = ctx.document.line_index.byte_range_to_range(
            &ctx.document.source,
            name_ident.start_byte(),
            name_ident.end_byte(),
        );
        ctx.diagnostics.push(WorkspaceDiagnostic {
            kind: KIND_WEAKER_ACCESS.to_string(),
            message: format!(
                "Function '{name}' cannot have a weaker access modifier than in \
                 ancestor class '{ancestor_class}'"
            ),
            severity: Severity::Error,
            range,
            related: vec![RelatedLocation {
                uri: ancestor.uri.clone(),
                range: ancestor.symbol.selection_range,
                message: format!("'{name}' declared here"),
            }],
            data: None,
        });
    }

    let own_count = declared_param_count(node);
    let ancestor_count = ctx
        .db
        .full_parameters_of(&ancestor.uri, ancestor.symbol.id)
        .len();
    if own_count != ancestor_count {
        let params_node = node.child_by_field_name(fields::PARAMS).unwrap_or(node);
        let range = ctx.document.line_index.byte_range_to_range(
            &ctx.document.source,
            params_node.start_byte(),
            params_node.end_byte(),
        );
        ctx.diagnostics.push(WorkspaceDiagnostic {
            kind: KIND_PARAM_COUNT.to_string(),
            message: format!(
                "Function '{name}' takes {own_count} parameter(s) which is inconsistent \
                 with base function ({ancestor_count})"
            ),
            severity: Severity::Error,
            range,
            related: vec![RelatedLocation {
                uri: ancestor.uri.clone(),
                range: ancestor.symbol.selection_range,
                message: format!("'{name}' declared here"),
            }],
            data: None,
        });
    }
    Some(())
}

fn declared_param_count(func_decl: Node) -> usize {
    let Some(params) = func_decl.child_by_field_name(fields::PARAMS) else {
        return 0;
    };
    let mut count = 0;
    let mut cursor = params.walk();
    for group in params.children(&mut cursor) {
        if group.kind() != kinds::FUNC_PARAM_GROUP {
            continue;
        }
        let mut group_cursor = group.walk();
        count += group
            .children_by_field_name(fields::NAMES, &mut group_cursor)
            .count();
    }
    count
}

#[cfg(test)]
mod tests;
