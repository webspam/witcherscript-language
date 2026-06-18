use lsp_types::request::{
    Completion, DocumentHighlightRequest, DocumentSymbolRequest, Formatting, GotoDefinition,
    GotoTypeDefinition, HoverRequest, InlayHintRequest, References, SemanticTokensFullRequest,
    SignatureHelpRequest, WorkspaceSymbolRequest,
};
use lsp_types::{
    CompletionParams, CompletionResponse, DocumentFormattingParams, DocumentHighlight,
    DocumentHighlightParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    FormattingOptions, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, InlayHint, InlayHintLabel, InlayHintParams, Location, NumberOrString, OneOf,
    PartialResultParams, Position, Range, ReferenceContext, ReferenceParams, SemanticToken,
    SemanticTokensParams, SemanticTokensResult, SemanticTokensServerCapabilities, SignatureHelp,
    SignatureHelpParams, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use super::EditorSession;
use super::model::{
    CompletionItemSnap, DiagSnap, HighlightSnap, HintSnap, HoverSnap, SignatureInfoSnap,
    SignatureSnap, SnapLoc, SymbolSnap, TextEditSnap, TokenSnap, WsSymbolSnap,
    completion_kind_name, fmt_pos, fmt_range, highlight_kind_name, inlay_kind_name, severity_name,
    symbol_kind_name,
};

impl EditorSession {
    pub(crate) async fn diagnostics(&mut self, rel: &str) -> Vec<DiagSnap> {
        let uri = self.uri_of(rel);
        let diags = self.client.pull_diagnostics(&uri).await;
        let mut out: Vec<DiagSnap> = diags
            .into_iter()
            .map(|d| DiagSnap {
                range: fmt_range(d.range),
                severity: severity_name(d.severity),
                code: d.code.map(|c| match c {
                    NumberOrString::Number(n) => n.to_string(),
                    NumberOrString::String(s) => s,
                }),
                message: self.workspace.redact_urls(&d.message),
            })
            .collect();
        out.sort_by(|a, b| (&a.range, &a.message).cmp(&(&b.range, &b.message)));
        out
    }

    pub(crate) async fn document_symbols(&mut self, rel: &str) -> Vec<SymbolSnap> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri_of(rel),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        match self
            .client
            .request_when_ready::<DocumentSymbolRequest>(params)
            .await
        {
            Some(DocumentSymbolResponse::Nested(syms)) => syms.iter().map(symbol_snap).collect(),
            Some(DocumentSymbolResponse::Flat(_)) => {
                panic!("server returned flat document symbols; expected nested")
            }
            None => Vec::new(),
        }
    }

    pub(crate) async fn semantic_tokens(&mut self, rel: &str) -> Vec<TokenSnap> {
        let (types, modifiers) = self.semantic_legend();
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri_of(rel),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let data = match self
            .client
            .request_when_ready::<SemanticTokensFullRequest>(params)
            .await
        {
            Some(SemanticTokensResult::Tokens(t)) => t.data,
            Some(SemanticTokensResult::Partial(_)) => panic!("unexpected partial semantic tokens"),
            None => Vec::new(),
        };
        decode_tokens(&data, &types, &modifiers)
    }

    pub(crate) async fn inlay_hints(&mut self, rel: &str) -> Vec<HintSnap> {
        let line_count = u32::try_from(self.file(rel).text.lines().count()).unwrap_or(u32::MAX);
        let params = InlayHintParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri_of(rel),
            },
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(line_count, 0),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let hints = self
            .client
            .request_when_ready::<InlayHintRequest>(params)
            .await
            .unwrap_or_default();
        hints.iter().map(|h| self.hint_snap(h)).collect()
    }

    pub(crate) async fn formatting(&mut self, rel: &str) -> Vec<TextEditSnap> {
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri_of(rel),
            },
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: false,
                ..FormattingOptions::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.client
            .request_when_ready::<Formatting>(params)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|e| TextEditSnap {
                range: fmt_range(e.range),
                new_text: e.new_text,
            })
            .collect()
    }

    pub(crate) async fn workspace_symbols(&mut self, query: &str) -> Vec<WsSymbolSnap> {
        let params = WorkspaceSymbolParams {
            query: query.to_string(),
            ..WorkspaceSymbolParams::default()
        };
        let mut out: Vec<WsSymbolSnap> = match self
            .client
            .request_when_ready::<WorkspaceSymbolRequest>(params)
            .await
        {
            Some(WorkspaceSymbolResponse::Nested(syms)) => syms
                .iter()
                .map(|s| {
                    let location = match &s.location {
                        OneOf::Left(loc) => self.loc(loc),
                        OneOf::Right(partial) => SnapLoc {
                            file: self.workspace.relativize(&partial.uri),
                            range: String::new(),
                        },
                    };
                    WsSymbolSnap {
                        name: s.name.clone(),
                        kind: symbol_kind_name(s.kind),
                        location,
                    }
                })
                .collect(),
            Some(WorkspaceSymbolResponse::Flat(syms)) => syms
                .iter()
                .map(|s| WsSymbolSnap {
                    name: s.name.clone(),
                    kind: symbol_kind_name(s.kind),
                    location: self.loc(&s.location),
                })
                .collect(),
            None => Vec::new(),
        };
        out.sort_by(|a, b| {
            (&a.name, &a.location.file, &a.location.range).cmp(&(
                &b.name,
                &b.location.file,
                &b.location.range,
            ))
        });
        out
    }

    pub(crate) async fn hover(&mut self, rel: &str) -> Option<HoverSnap> {
        let params = HoverParams {
            text_document_position_params: self.doc_pos(rel),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let hover: Option<Hover> = self.client.request_when_ready::<HoverRequest>(params).await;
        hover.map(|h| HoverSnap {
            range: h.range.map(fmt_range),
            value: self.workspace.redact_urls(&hover_value(&h.contents)),
        })
    }

    pub(crate) async fn definition(&mut self, rel: &str) -> Vec<SnapLoc> {
        let params = goto_params(self.doc_pos(rel));
        let resp = self
            .client
            .request_when_ready::<GotoDefinition>(params)
            .await;
        self.goto_locations(resp)
    }

    pub(crate) async fn type_definition(&mut self, rel: &str) -> Vec<SnapLoc> {
        let params = goto_params(self.doc_pos(rel));
        let resp = self
            .client
            .request_when_ready::<GotoTypeDefinition>(params)
            .await;
        self.goto_locations(resp)
    }

    pub(crate) async fn references(&mut self, rel: &str) -> Vec<SnapLoc> {
        let params = ReferenceParams {
            text_document_position: self.doc_pos(rel),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        };
        let locations = self
            .client
            .request_when_ready::<References>(params)
            .await
            .unwrap_or_default();
        let mut out: Vec<SnapLoc> = locations.iter().map(|l| self.loc(l)).collect();
        out.sort_by(|a, b| (&a.file, &a.range).cmp(&(&b.file, &b.range)));
        out
    }

    pub(crate) async fn highlights(&mut self, rel: &str) -> Vec<HighlightSnap> {
        let params = DocumentHighlightParams {
            text_document_position_params: self.doc_pos(rel),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let highlights: Vec<DocumentHighlight> = self
            .client
            .request_when_ready::<DocumentHighlightRequest>(params)
            .await
            .unwrap_or_default();
        let mut out: Vec<HighlightSnap> = highlights
            .iter()
            .map(|h| HighlightSnap {
                range: fmt_range(h.range),
                kind: highlight_kind_name(h.kind),
            })
            .collect();
        out.sort_by(|a, b| a.range.cmp(&b.range));
        out
    }

    pub(crate) async fn completion(&mut self, rel: &str) -> Vec<CompletionItemSnap> {
        let params = CompletionParams {
            text_document_position: self.doc_pos(rel),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        let items = match self.client.request_when_ready::<Completion>(params).await {
            Some(CompletionResponse::Array(items)) => items,
            Some(CompletionResponse::List(list)) => list.items,
            None => Vec::new(),
        };
        let mut out: Vec<CompletionItemSnap> = items
            .into_iter()
            .map(|i| CompletionItemSnap {
                label: i.label,
                kind: i.kind.map(completion_kind_name),
                detail: i.detail.map(|d| self.workspace.redact_urls(&d)),
            })
            .collect();
        out.sort_by(|a, b| (&a.label, &a.detail).cmp(&(&b.label, &b.detail)));
        out
    }

    pub(crate) async fn signature_help(&mut self, rel: &str) -> Option<SignatureSnap> {
        let params = SignatureHelpParams {
            text_document_position_params: self.doc_pos(rel),
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        };
        let help: Option<SignatureHelp> = self
            .client
            .request_when_ready::<SignatureHelpRequest>(params)
            .await;
        help.map(|h| SignatureSnap {
            active_signature: h.active_signature,
            active_parameter: h.active_parameter,
            signatures: h
                .signatures
                .into_iter()
                .map(|s| SignatureInfoSnap {
                    label: self.workspace.redact_urls(&s.label),
                    parameters: s
                        .parameters
                        .unwrap_or_default()
                        .into_iter()
                        .map(|p| match p.label {
                            lsp_types::ParameterLabel::Simple(text) => text,
                            lsp_types::ParameterLabel::LabelOffsets([start, end]) => {
                                format!("{start}..{end}")
                            }
                        })
                        .collect(),
                })
                .collect(),
        })
    }

    pub(crate) async fn edit(&mut self, rel: &str, version: i32, new_text: &str) {
        let uri = self.uri_of(rel);
        self.client.change_full(&uri, version, new_text).await;
        self.client.wait_until_indexed().await;
    }

    fn doc_pos(&self, rel: &str) -> TextDocumentPositionParams {
        TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri_of(rel),
            },
            position: self.cursor_in(rel),
        }
    }

    fn loc(&self, location: &Location) -> SnapLoc {
        SnapLoc {
            file: self.workspace.relativize(&location.uri),
            range: fmt_range(location.range),
        }
    }

    fn goto_locations(&self, resp: Option<GotoDefinitionResponse>) -> Vec<SnapLoc> {
        let mut locs = match resp {
            Some(GotoDefinitionResponse::Scalar(l)) => vec![self.loc(&l)],
            Some(GotoDefinitionResponse::Array(ls)) => ls.iter().map(|l| self.loc(l)).collect(),
            Some(GotoDefinitionResponse::Link(links)) => links
                .iter()
                .map(|l| SnapLoc {
                    file: self.workspace.relativize(&l.target_uri),
                    range: fmt_range(l.target_selection_range),
                })
                .collect(),
            None => Vec::new(),
        };
        locs.sort_by(|a, b| (&a.file, &a.range).cmp(&(&b.file, &b.range)));
        locs
    }

    fn hint_snap(&self, hint: &InlayHint) -> HintSnap {
        let label = match &hint.label {
            InlayHintLabel::String(s) => s.clone(),
            InlayHintLabel::LabelParts(parts) => {
                parts.iter().map(|p| p.value.clone()).collect::<String>()
            }
        };
        HintSnap {
            position: fmt_pos(hint.position),
            label: self.workspace.redact_urls(&label),
            kind: inlay_kind_name(hint.kind),
        }
    }

    fn semantic_legend(&self) -> (Vec<String>, Vec<String>) {
        let legend = match self
            .client
            .server_capabilities()
            .semantic_tokens_provider
            .as_ref()
        {
            Some(SemanticTokensServerCapabilities::SemanticTokensOptions(o)) => &o.legend,
            Some(SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(o)) => {
                &o.semantic_tokens_options.legend
            }
            None => panic!("server does not advertise semantic tokens"),
        };
        (
            legend
                .token_types
                .iter()
                .map(|t| t.as_str().to_string())
                .collect(),
            legend
                .token_modifiers
                .iter()
                .map(|m| m.as_str().to_string())
                .collect(),
        )
    }
}

fn goto_params(position: TextDocumentPositionParams) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: position,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn hover_value(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Markup(markup) => markup.value.clone(),
        HoverContents::Scalar(marked) => marked_string_text(marked),
        HoverContents::Array(parts) => parts
            .iter()
            .map(marked_string_text)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn marked_string_text(marked: &lsp_types::MarkedString) -> String {
    match marked {
        lsp_types::MarkedString::String(s) => s.clone(),
        lsp_types::MarkedString::LanguageString(ls) => ls.value.clone(),
    }
}

fn symbol_snap(symbol: &DocumentSymbol) -> SymbolSnap {
    SymbolSnap {
        name: symbol.name.clone(),
        kind: symbol_kind_name(symbol.kind),
        range: fmt_range(symbol.range),
        selection: fmt_range(symbol.selection_range),
        detail: symbol.detail.clone(),
        children: symbol
            .children
            .as_ref()
            .map(|c| c.iter().map(symbol_snap).collect())
            .unwrap_or_default(),
    }
}

fn decode_tokens(data: &[SemanticToken], types: &[String], modifiers: &[String]) -> Vec<TokenSnap> {
    let mut out = Vec::with_capacity(data.len());
    let mut line = 0u32;
    let mut start = 0u32;
    for token in data {
        if token.delta_line == 0 {
            start += token.delta_start;
        } else {
            line += token.delta_line;
            start = token.delta_start;
        }
        let token_type = types
            .get(token.token_type as usize)
            .cloned()
            .unwrap_or_else(|| format!("type#{}", token.token_type));
        out.push(TokenSnap {
            range: format!("{line}:{start}-{line}:{}", start + token.length),
            token_type,
            modifiers: decode_modifiers(token.token_modifiers_bitset, modifiers),
        });
    }
    out
}

fn decode_modifiers(bitset: u32, modifiers: &[String]) -> Vec<String> {
    (0..modifiers.len())
        .filter(|i| bitset & (1 << i) != 0)
        .map(|i| modifiers[i].clone())
        .collect()
}
