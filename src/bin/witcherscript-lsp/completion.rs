use std::time::Instant;

use async_lsp::ResponseError;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    CompletionTriggerKind, InsertTextFormat,
};
use tracing::trace;
use witcherscript_language::files::canonical_uri;
use witcherscript_language::resolve::{
    after_wrap_method_completions, annotation_arg_completions, annotation_name_completions,
    class_body_keyword_completions, class_header_keyword_completions, completion_members,
    default_or_hint_member_completions, expression_completions, extends_completions,
    new_lifetime_completions, new_type_completions, position_in_comment, script_body_completions,
    state_owner_completions, statement_completions, type_completions_arc, Definition, SymbolDb,
    BUILTIN_TYPE_COMPLETIONS,
};

use crate::backend::Backend;
use crate::convert::{
    annotation_name_items, builtin_type_item, class_body_kw_item, completion_item,
    keyword_snippet_item, lsp_range, script_body_item, source_position, source_range,
    this_super_item, type_completion_item, wrap_method_snippet,
};

type Result<T> = std::result::Result<T, ResponseError>;

fn triggered_by_dot(params: &CompletionParams) -> bool {
    let Some(ctx) = &params.context else {
        return false;
    };
    ctx.trigger_kind == CompletionTriggerKind::TRIGGER_CHARACTER
        && ctx.trigger_character.as_deref() == Some(".")
}

fn sorted_completion_item(db: &SymbolDb, def: &Definition, tier: u8) -> CompletionItem {
    let params = db.parameters_of(&def.uri, def.symbol.id);
    let mut item = completion_item(def, &params);
    item.sort_text = Some(format!("{}_{}", tier, def.symbol.name));
    item
}

impl Backend {
    pub(crate) fn _completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let dot_triggered = triggered_by_dot(&params);
        let trigger_kind = params.context.as_ref().map(|c| c.trigger_kind);
        let trigger_char = params
            .context
            .as_ref()
            .and_then(|c| c.trigger_character.clone());
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let started_at = Instant::now();
        trace!(
            op = "completion",
            uri = %uri,
            line = position.line,
            character = position.character,
            trigger_kind = ?trigger_kind,
            trigger_char = ?trigger_char,
            "start",
        );
        let result: Result<Option<CompletionResponse>> = 'body: {
            let snap = self.snapshot();
            // A typed `.` arrives as a queued edit; parse it now so completion sees the dot, not the stale tree.
            let Some(document_arc) = self.latest_parsed_document(&uri) else {
                trace!(op = "completion", "no document for uri");
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();
            let canonical = canonical_uri(&uri);

            let pos = source_position(position);

            let byte_offset = document.line_index.position_to_byte(&document.source, pos);
            let text_before_cursor = byte_offset
                .and_then(|off| document.source.get(off.saturating_sub(16)..off))
                .unwrap_or("");
            trace!(
                op = "completion",
                parse_version = document.parse_version,
                byte_offset = ?byte_offset,
                text_before_cursor,
                "document state seen by request",
            );

            if position_in_comment(document, pos) {
                trace!(op = "completion", "cursor in comment, suppressing");
                break 'body Ok(None);
            }

            let member_items: Vec<CompletionItem> =
                completion_members(&canonical, document, &db, pos)
                    .iter()
                    .map(|(tier, def)| sorted_completion_item(&db, def, *tier))
                    .collect();
            trace!(
                op = "completion",
                member_count = member_items.len(),
                "members resolved"
            );
            if !member_items.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(member_items)));
            }

            // A `.` keypress only ever opens a member access; suppress the statement/keyword fall-through.
            if dot_triggered {
                trace!(
                    op = "completion",
                    "dot trigger with no members, suppressing"
                );
                break 'body Ok(None);
            }

            let default_or_hint = default_or_hint_member_completions(document, &db, pos);
            if !default_or_hint.is_empty() {
                let items: Vec<CompletionItem> = default_or_hint
                    .iter()
                    .map(|def| sorted_completion_item(&db, def, 0))
                    .collect();
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            let annotation_arg = annotation_arg_completions(document, &db, pos);
            if !annotation_arg.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(
                    annotation_arg.iter().map(type_completion_item).collect(),
                )));
            }

            if let Some(at_pos) = annotation_name_completions(document, pos) {
                let replace_range = lsp_range(source_range(at_pos, pos));
                break 'body Ok(Some(CompletionResponse::Array(annotation_name_items(
                    replace_range,
                ))));
            }

            if let Some(wrap) = after_wrap_method_completions(document, &db, pos) {
                let items = wrap
                    .methods
                    .iter()
                    .map(|def| {
                        let snippet = wrap_method_snippet(def, &db);
                        let insert_text = if wrap.needs_function_keyword {
                            format!("function {snippet}")
                        } else {
                            snippet
                        };
                        CompletionItem {
                            label: def.symbol.name.clone(),
                            kind: Some(CompletionItemKind::METHOD),
                            detail: def.symbol.signature.clone(),
                            insert_text: Some(insert_text),
                            insert_text_format: Some(InsertTextFormat::SNIPPET),
                            ..CompletionItem::default()
                        }
                    })
                    .collect();
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            let extends = extends_completions(document, &db, pos);
            if !extends.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(
                    extends.iter().map(type_completion_item).collect(),
                )));
            }

            let state_owners = state_owner_completions(document, &db, pos);
            if !state_owners.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(
                    state_owners.iter().map(type_completion_item).collect(),
                )));
            }

            let header_kws = class_header_keyword_completions(document, pos);
            if !header_kws.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(
                    header_kws
                        .iter()
                        .map(|kw| keyword_snippet_item(kw, &format!("{kw} ")))
                        .collect(),
                )));
            }

            let new_types = new_type_completions(&canonical, document, &db, pos);
            if !new_types.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(
                    new_types.iter().map(type_completion_item).collect(),
                )));
            }

            if type_completions_arc(document, &db, pos).is_some() {
                let merged_cache = self.merged_completion_cache(&uri, &handles);
                let mut items: Vec<CompletionItem> = BUILTIN_TYPE_COMPLETIONS
                    .iter()
                    .map(|name| builtin_type_item(name))
                    .collect();
                items.extend(merged_cache.types.iter().map(type_completion_item));
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            let class_body_kws = class_body_keyword_completions(document, pos);
            if !class_body_kws.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(
                    class_body_kws
                        .iter()
                        .map(|kw| class_body_kw_item(kw))
                        .collect(),
                )));
            }

            let script_body_kws = script_body_completions(document, pos);
            if !script_body_kws.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(
                    script_body_kws
                        .iter()
                        .map(|kw| script_body_item(kw))
                        .collect(),
                )));
            }

            let new_lifetime = new_lifetime_completions(&canonical, document, &db, pos);
            if !new_lifetime.is_empty() {
                let items: Vec<CompletionItem> = new_lifetime
                    .iter()
                    .map(|def| sorted_completion_item(&db, def, 0))
                    .collect();
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            let stmt = statement_completions(&canonical, document, &db, pos);
            if stmt.active {
                let merged_cache = stmt
                    .needs_globals
                    .then(|| self.merged_completion_cache(&uri, &handles));
                let stmt_globals = merged_cache.as_ref().map(|c| c.globals()).unwrap_or(&[]);
                let mut items: Vec<CompletionItem> = Vec::new();
                if stmt.has_this {
                    items.push(this_super_item("this"));
                }
                if stmt.has_super {
                    items.push(this_super_item("super"));
                }
                items.push(keyword_snippet_item("var", "var ${1:name} : ${2:Type};"));
                items.push(keyword_snippet_item("if", "if (${1:condition})"));
                items.push(keyword_snippet_item("else", "else"));
                items.push(keyword_snippet_item("return", "return;"));
                items.push(keyword_snippet_item(
                    "for",
                    "for (${1:init}; ${2:condition}; ${3:increment}) {\n\t$0\n}",
                ));
                items.push(keyword_snippet_item(
                    "while",
                    "while (${1:condition}) {\n\t$0\n}",
                ));
                items.push(keyword_snippet_item(
                    "do",
                    "do {\n\t$0\n} while (${1:condition});",
                ));
                items.push(keyword_snippet_item(
                    "switch",
                    "switch (${1:expr}) {\n\tcase ${2:val}:\n\t\t$0\n\t\tbreak;\n}",
                ));
                if stmt.in_switch {
                    items.push(keyword_snippet_item("case", "case ${1:val}: $0"));
                    items.push(keyword_snippet_item("default", "default: $0"));
                    items.push(keyword_snippet_item("break", "break;"));
                }
                if stmt.in_loop {
                    items.push(keyword_snippet_item("break", "break;"));
                    items.push(keyword_snippet_item("continue", "continue;"));
                }
                for def in &stmt.locals {
                    items.push(sorted_completion_item(&db, def, 0));
                }
                for def in &stmt.members {
                    items.push(sorted_completion_item(&db, def, 1));
                }
                for def in stmt_globals {
                    items.push(sorted_completion_item(&db, def, 2));
                }
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            if let Some(expr) = expression_completions(&canonical, document, &db, pos) {
                let merged_cache = expr
                    .needs_globals
                    .then(|| self.merged_completion_cache(&uri, &handles));
                let expr_globals = merged_cache.as_ref().map(|c| c.globals()).unwrap_or(&[]);
                let mut items: Vec<CompletionItem> = Vec::new();
                if expr.has_this {
                    items.push(this_super_item("this"));
                }
                if expr.has_super {
                    items.push(this_super_item("super"));
                }
                items.push(keyword_snippet_item("true", "true"));
                items.push(keyword_snippet_item("false", "false"));
                for def in &expr.locals {
                    items.push(sorted_completion_item(&db, def, 0));
                }
                for def in &expr.members {
                    items.push(sorted_completion_item(&db, def, 0));
                }
                for def in expr_globals {
                    items.push(sorted_completion_item(&db, def, 2));
                }
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            Ok(None)
        };
        trace!(
            op = "completion",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            "complete",
        );
        result
    }
}
