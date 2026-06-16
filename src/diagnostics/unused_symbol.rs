use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::literals::is_constant_literal;
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

fn check_unused<'tree>(node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
    // @addField injects a field into another class; its uses live in that class, not here.
    if node.child_by_field_name(fields::ANNOTATION).is_some() {
        return;
    }

    let mut cursor = node.walk();
    // The `names` field spans the comma-separated list, so the separators land in it too.
    let names: Vec<Node> = node
        .children_by_field_name(fields::NAMES, &mut cursor)
        .filter(|n| n.kind() == kinds::IDENT)
        .collect();
    if names.is_empty() {
        return;
    }

    // A `default`/`hint` initialises a field rather than using it, so it must not count as a reference.
    let initialiser_ranges = if node.kind() == kinds::MEMBER_VAR_DECL {
        field_initialiser_ranges(node)
    } else {
        Vec::new()
    };

    let mut unused: Vec<(Node, String)> = Vec::new();
    for ident in &names {
        let Ok(name) = ident.utf8_text(ctx.document.source.as_bytes()) else {
            continue;
        };
        let Some(symbol) = declared_symbol(ctx.document, node, *ident, name) else {
            continue;
        };
        // Only private fields are local enough to call unused; public/protected are the type's API.
        if symbol.kind == SymbolKind::Field && symbol.access != AccessLevel::Private {
            continue;
        }
        if is_referenced(symbol, &initialiser_ranges, ctx) {
            continue;
        }
        unused.push((*ident, name.to_string()));
    }
    if unused.is_empty() {
        return;
    }

    if node.kind() == kinds::FUNC_PARAM_GROUP {
        if is_bodyless_func(node) {
            return;
        }
        emit_param_dims(node, &names, &unused, ctx);
    } else {
        emit_var_decl_dims(node, &names, &unused, ctx);
    }
}

fn is_bodyless_func(param_group: Node<'_>) -> bool {
    let func_decl = param_group.parent().and_then(|p| p.parent());
    func_decl
        .and_then(|f| f.child_by_field_name(fields::DEFINITION))
        .is_some_and(|def| def.kind() == kinds::NOP)
}

fn emit_param_dims<'tree>(
    node: Node<'tree>,
    names: &[Node<'tree>],
    unused: &[(Node<'tree>, String)],
    ctx: &mut CstRuleCtx<'_, 'tree>,
) {
    // Whole group dead: fade specifiers, names, `:`, and type together. A lone param is this case too.
    if unused.len() == names.len() {
        let message = if let [(_, name)] = unused {
            format!("Parameter '{name}' is never used")
        } else {
            let list: Vec<String> = unused.iter().map(|(_, n)| format!("'{n}'")).collect();
            format!("Parameters {} are never used", list.join(", "))
        };
        push_dim(ctx, node.start_byte(), node.end_byte(), message);
        return;
    }

    // Only some names dead; the group shares specifiers and type, so each fades on its own.
    for (ident, name) in unused {
        push_dim(
            ctx,
            ident.start_byte(),
            ident.end_byte(),
            format!("Parameter '{name}' is never used"),
        );
    }
}

fn emit_var_decl_dims<'tree>(
    node: Node<'tree>,
    names: &[Node<'tree>],
    unused: &[(Node<'tree>, String)],
    ctx: &mut CstRuleCtx<'_, 'tree>,
) {
    let (singular, plural) = var_decl_nouns(node.kind());

    if unused.len() == names.len() {
        let literal_init = node
            .child_by_field_name(fields::INIT_VALUE)
            .is_some_and(is_constant_literal);
        // The whole declaration is dead; a computed initialiser stays bright, a literal one fades too.
        let end = match assignment_token(node) {
            Some(eq) if !literal_init => eq.start_byte(),
            _ => node.end_byte(),
        };
        let message = if let [(_, name)] = unused {
            format!("{singular} '{name}' is never used")
        } else {
            let list: Vec<String> = unused.iter().map(|(_, n)| format!("'{n}'")).collect();
            format!("{plural} {} are never used", list.join(", "))
        };
        push_dim(ctx, node.start_byte(), end, message);
        return;
    }

    for (ident, name) in unused {
        let end = match ident.next_sibling() {
            Some(comma) if comma.kind() == "," => comma.end_byte(),
            _ => ident.end_byte(),
        };
        push_dim(
            ctx,
            ident.start_byte(),
            end,
            format!("{singular} '{name}' is never used"),
        );
    }
}

fn var_decl_nouns(kind: &str) -> (&'static str, &'static str) {
    match kind {
        kinds::LOCAL_VAR_DECL_STMT => ("Local variable", "Local variables"),
        _ => ("Field", "Fields"),
    }
}

fn assignment_token(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == "=")
}

fn field_initialiser_ranges(field: Node<'_>) -> Vec<Range<usize>> {
    let Some(body) = field.parent() else {
        return Vec::new();
    };
    let mut cursor = body.walk();
    body.children(&mut cursor)
        .filter(|c| {
            matches!(
                c.kind(),
                kinds::MEMBER_DEFAULT_VAL
                    | kinds::MEMBER_DEFAULT_VAL_BLOCK
                    | kinds::MEMBER_DEFAULT_VAL_BLOCK_ASSIGN
                    | kinds::MEMBER_HINT
            )
        })
        .map(|c| c.byte_range())
        .collect()
}

fn is_referenced(
    symbol: Symbol,
    initialiser_ranges: &[Range<usize>],
    ctx: &CstRuleCtx<'_, '_>,
) -> bool {
    let definition = Definition {
        uri: ctx.uri.to_string(),
        symbol,
    };
    let refs = find_references(
        &definition,
        ctx.document,
        &[(ctx.uri, ctx.document)],
        ctx.db,
        false,
    );
    refs.iter().any(|(_, range)| {
        match ctx
            .document
            .line_index
            .position_to_byte(&ctx.document.source, range.start)
        {
            Some(byte) => !initialiser_ranges.iter().any(|r| r.contains(&byte)),
            None => true,
        }
    })
}

fn push_dim(ctx: &mut CstRuleCtx<'_, '_>, start: usize, end: usize, message: String) {
    let range = ctx
        .document
        .line_index
        .byte_range_to_range(&ctx.document.source, start, end);
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: KIND.to_string(),
        message,
        severity: Severity::Hint,
        range,
        related: Vec::new(),
        data: None,
    });
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
