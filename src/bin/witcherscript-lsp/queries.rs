use async_lsp::ResponseError;
use lsp_types::{
    CodeActionParams, CodeActionResponse, DocumentFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, Location, MarkupContent, MarkupKind, SemanticToken, SemanticTokens,
    SemanticTokensParams, SemanticTokensResult, SignatureHelp, SignatureHelpParams, TextEdit, Url,
};
use witcherscript_language::builtins::builtin_source;
use witcherscript_language::formatter::format_document;
use witcherscript_language::resolve::{
    resolve_all_definitions, resolve_definition, signature_help,
};
use witcherscript_language::semantic_tokens::collect_semantic_tokens;

use crate::backend::Backend;
use crate::convert::{
    base_script_conflict_code_actions, document_symbols, hover_markdown, lsp_range,
    signature_help_response, source_position,
};

type Result<T> = std::result::Result<T, ResponseError>;

impl Backend {
    pub(crate) async fn _code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let roots = self.workspace_roots.lock().clone();
        let actions = base_script_conflict_code_actions(&params.context.diagnostics, &roots);
        Ok((!actions.is_empty()).then_some(actions))
    }

    pub(crate) async fn _definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let documents = self.documents.lock();
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri);
        let db = handles.db();
        let definitions =
            resolve_all_definitions(uri.as_str(), document, &db, source_position(position));

        let locations: Vec<Location> = definitions
            .into_iter()
            .filter_map(|definition| {
                Url::parse(&definition.uri).ok().map(|target_uri| Location {
                    uri: target_uri,
                    range: lsp_range(definition.symbol.selection_range),
                })
            })
            .collect();

        match locations.as_slice() {
            [] => Ok(None),
            [single] => Ok(Some(GotoDefinitionResponse::Scalar(single.clone()))),
            _ => Ok(Some(GotoDefinitionResponse::Array(locations))),
        }
    }

    pub(crate) async fn _hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let documents = self.documents.lock();
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri);
        let db = handles.db();
        let Some(definition) =
            resolve_definition(uri.as_str(), document, &db, source_position(position))
        else {
            return Ok(None);
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_markdown(&definition),
            }),
            range: Some(lsp_range(definition.symbol.selection_range)),
        }))
    }

    pub(crate) async fn _signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let documents = self.documents.lock();
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri);
        let db = handles.db();
        let compact_colon = self.config.load().formatter_compact_colon;

        Ok(signature_help(
            uri.as_str(),
            document,
            &db,
            source_position(position),
            compact_colon,
        )
        .map(signature_help_response))
    }

    pub(crate) async fn _document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let documents = self.documents.lock();
        let Some(document) = documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        Ok(Some(DocumentSymbolResponse::Nested(document_symbols(
            &document.symbols,
            None,
            params.text_document.uri.as_str(),
        ))))
    }

    pub(crate) async fn _semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let documents = self.documents.lock();
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let handles = self.db_handles_for(&uri);
        let db = handles.db();
        let data = collect_semantic_tokens(uri.as_str(), document, &db);
        let tokens: Vec<SemanticToken> = data
            .chunks_exact(5)
            .map(|c| SemanticToken {
                delta_line: c[0],
                delta_start: c[1],
                length: c[2],
                token_type: c[3],
                token_modifiers_bitset: c[4],
            })
            .collect();
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    pub(crate) async fn _formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        if builtin_source(uri.as_str()).is_some() {
            return Ok(None);
        }
        let tab_size = params.options.tab_size;
        let use_tabs = !params.options.insert_spaces;

        let documents = self.documents.lock();
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };

        let cfg = self.config.load();
        let line_limit = cfg.formatter_line_limit;
        let compact_colon = cfg.formatter_compact_colon;
        let align_member_colons = cfg.formatter_align_member_colons;

        let formatted = format_document(
            document.tree.root_node(),
            &document.source,
            tab_size,
            use_tabs,
            line_limit,
            compact_colon,
            align_member_colons,
        );

        if formatted == document.source {
            return Ok(Some(Vec::new()));
        }

        let full_range = lsp_range(document.line_index.byte_range_to_range(
            &document.source,
            0,
            document.source.len(),
        ));

        Ok(Some(vec![TextEdit {
            range: full_range,
            new_text: formatted,
        }]))
    }
}
