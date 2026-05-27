use std::time::Instant;

use async_lsp::ResponseError;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, InsertTextFormat,
};
use tracing::trace;
use witcherscript_language::resolve::{
    after_wrap_method_completions, annotation_arg_completions, annotation_name_completions,
    class_body_keyword_completions, class_header_keyword_completions, completion_members,
    default_or_hint_member_completions, expression_completions, extends_completions,
    new_lifetime_completions, new_type_completions, script_body_completions,
    state_owner_completions, statement_completions, type_completions_arc,
    AfterWrapMethodCompletions, BUILTIN_TYPE_COMPLETIONS,
};

use crate::backend::Backend;
use crate::convert::{
    annotation_name_items, builtin_type_item, class_body_kw_item, completion_item,
    keyword_snippet_item, lsp_range, script_body_item, source_position, source_range,
    this_super_item, type_completion_item, wrap_method_snippet,
};
use crate::logging::wall_clock_us;

type Result<T> = std::result::Result<T, ResponseError>;

impl Backend {
    pub(crate) async fn _completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let started_at = Instant::now();
        trace!(op = "completion", uri = %uri, at = %wall_clock_us(), "start");
        let result: Result<Option<CompletionResponse>> = 'body: {
            let snap = self.snapshot();
            let Some(document_arc) = snap.documents.get(&uri).cloned() else {
                break 'body Ok(None);
            };
            let document = document_arc.as_ref();
            let handles = self.db_handles_for_with_snapshot(&uri, &snap);
            let db = handles.db();

            let pos = source_position(position);

            let member_items: Vec<CompletionItem> =
                completion_members(uri.as_str(), document, &db, pos)
                    .iter()
                    .map(|(tier, def)| {
                        let params = db.parameters_of(&def.uri, def.symbol.id);
                        let mut item = completion_item(def, &params);
                        item.sort_text = Some(format!("{}_{}", tier, def.symbol.name));
                        item
                    })
                    .collect();
            if !member_items.is_empty() {
                break 'body Ok(Some(CompletionResponse::Array(member_items)));
            }

            let default_or_hint = default_or_hint_member_completions(document, &db, pos);
            if !default_or_hint.is_empty() {
                let items: Vec<CompletionItem> = default_or_hint
                    .iter()
                    .map(|def| {
                        let params = db.parameters_of(&def.uri, def.symbol.id);
                        let mut item = completion_item(def, &params);
                        item.sort_text = Some(format!("0_{}", def.symbol.name));
                        item
                    })
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

            match after_wrap_method_completions(document, &db, pos) {
                Some(AfterWrapMethodCompletions::FunctionKeyword) => {
                    break 'body Ok(Some(CompletionResponse::Array(vec![keyword_snippet_item(
                        "function", "function",
                    )])));
                }
                Some(AfterWrapMethodCompletions::MethodList(methods)) => {
                    let items = methods
                        .iter()
                        .map(|def| {
                            let snippet = wrap_method_snippet(def, &db);
                            CompletionItem {
                                label: def.symbol.name.clone(),
                                kind: Some(CompletionItemKind::METHOD),
                                detail: def.symbol.signature.clone(),
                                insert_text: Some(snippet),
                                insert_text_format: Some(InsertTextFormat::SNIPPET),
                                ..CompletionItem::default()
                            }
                        })
                        .collect();
                    break 'body Ok(Some(CompletionResponse::Array(items)));
                }
                None => {}
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

            let new_types = new_type_completions(uri.as_str(), document, &db, pos);
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

            let new_lifetime = new_lifetime_completions(uri.as_str(), document, &db, pos);
            if !new_lifetime.is_empty() {
                let items: Vec<CompletionItem> = new_lifetime
                    .iter()
                    .map(|def| {
                        let params = db.parameters_of(&def.uri, def.symbol.id);
                        let mut item = completion_item(def, &params);
                        item.sort_text = Some(format!("0_{}", def.symbol.name));
                        item
                    })
                    .collect();
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            let stmt = statement_completions(uri.as_str(), document, &db, pos);
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
                    let params = db.parameters_of(&def.uri, def.symbol.id);
                    let mut item = completion_item(def, &params);
                    item.sort_text = Some(format!("0_{}", def.symbol.name));
                    items.push(item);
                }
                for def in &stmt.members {
                    let params = db.parameters_of(&def.uri, def.symbol.id);
                    let mut item = completion_item(def, &params);
                    item.sort_text = Some(format!("1_{}", def.symbol.name));
                    items.push(item);
                }
                for def in stmt_globals {
                    let params = db.parameters_of(&def.uri, def.symbol.id);
                    let mut item = completion_item(def, &params);
                    item.sort_text = Some(format!("2_{}", def.symbol.name));
                    items.push(item);
                }
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            if let Some(expr) = expression_completions(uri.as_str(), document, &db, pos) {
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
                    let params = db.parameters_of(&def.uri, def.symbol.id);
                    let mut item = completion_item(def, &params);
                    item.sort_text = Some(format!("0_{}", def.symbol.name));
                    items.push(item);
                }
                for def in &expr.members {
                    let params = db.parameters_of(&def.uri, def.symbol.id);
                    let mut item = completion_item(def, &params);
                    item.sort_text = Some(format!("0_{}", def.symbol.name));
                    items.push(item);
                }
                for def in expr_globals {
                    let params = db.parameters_of(&def.uri, def.symbol.id);
                    let mut item = completion_item(def, &params);
                    item.sort_text = Some(format!("2_{}", def.symbol.name));
                    items.push(item);
                }
                break 'body Ok(Some(CompletionResponse::Array(items)));
            }

            Ok(None)
        };
        trace!(
            op = "completion",
            uri = %uri,
            elapsed_us = started_at.elapsed().as_micros(),
            at = %wall_clock_us(),
            "complete",
        );
        result
    }
}
